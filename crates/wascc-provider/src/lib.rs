//! A custom kubelet backend that can run [waSCC](https://wascc.dev/) based workloads
//!
//! The crate provides the [`WasccProvider`] type which can be used
//! as a provider with [`kubelet`].
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Kubelet, config::Config};
//! use kubelet::module_store::FileModuleStore;
//! use wascc_provider::WasccProvider;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!     let client = oci_distribution::Client::default();
//!     let store = FileModuleStore::new(client, &std::path::PathBuf::from(""));
//!
//!     // Instantiate the provider type
//!     let provider = WasccProvider::new(store, &kubelet_config).await.unwrap();
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::config::load_kube_config().await.unwrap();
//!     
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config);
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! }
//! ```

#![warn(missing_docs)]

use async_trait::async_trait;
use kubelet::module_store::ModuleStore;
use kubelet::provider::ProviderError;
use kubelet::status::{ContainerStatus};
use kubelet::{Pod, Provider};
use kubelet::handle::{PodHandle, RuntimeHandle, Stop, key_from_pod, pod_key};
use log::{error,debug, info, warn};
use wascc_host::{host, Actor, NativeCapability};
use tokio::sync::RwLock;
use tokio::fs::File;
use tokio::sync::watch::{self, Receiver};
use tempfile::NamedTempFile;

use wascc_logging::{LOG_PATH_KEY};

use std::collections::HashMap;
use std::path::{PathBuf, Path};
use std::sync::Arc;

/// The architecture that the pod targets.
const TARGET_WASM32_WASCC: &str = "wasm32-wascc";

/// The name of the HTTP capability.
const HTTP_CAPABILITY: &str = "wascc:http_server";

/// The name of the Logging capability.
const LOG_CAPABILITY: &str = "wascc:logging";

/// The root directory of waSCC logs.
const LOG_DIR_NAME: &str = "wascc-logs";

#[cfg(target_os = "linux")]
const HTTP_LIB: &str = "./lib/libwascc_httpsrv.so";

#[cfg(target_os = "linux")]
const LOG_LIB: &str = "./lib/libwascc_logging.so";

#[cfg(target_os = "macos")]
const HTTP_LIB: &str = "./lib/libwascc_httpsrv.dylib";

#[cfg(target_os = "macos")]
const LOG_LIB: &str = "./lib/libwascc_logging.dylib";

/// Kubernetes' view of environment variables is an unordered map of string to string.
type EnvVars = std::collections::HashMap<String, String>;

/// A [kubelet::handle::Stop] implementation for a wascc actor
pub struct ActorStopper {
    /// The public key of the wascc Actor that will be stopped
    pub key: String,
}

#[async_trait::async_trait]
impl Stop for ActorStopper {
    async fn stop(&mut self) -> anyhow::Result<()> {
        debug!("stopping wascc instance {}", self.key);
        host::remove_actor(&self.key).map_err(|e| anyhow::anyhow!("unable to remove actor: {:?}", e))
    }

    async fn wait(&mut self) -> anyhow::Result<()> {
        // TODO: Figure out if there is a way to wait for an actor to be removed
        Ok(())
    }
}

/// WasccProvider provides a Kubelet runtime implementation that executes WASM binaries.
///
/// Currently, this runtime uses WASCC as a host, loading the primary container as an actor.
/// TODO: In the future, we will look at loading capabilities using the "sidecar" metaphor
/// from Kubernetes.
#[derive(Clone)]
pub struct WasccProvider<S> {
    handles: Arc<RwLock<HashMap<String, PodHandle<ActorStopper, File>>>>,
    store: S,
    log_path: PathBuf,
    kubeconfig: kube::config::Configuration,
}

impl<S: ModuleStore + Send + Sync> WasccProvider<S> {
    /// Returns a new wasCC provider configured to use the proper data directory
    /// (including creating it if necessary)
    pub async fn new(store: S, config: &kubelet::config::Config, kubeconfig: kube::config::Configuration) -> anyhow::Result<Self> {
        let log_path = config.data_dir.to_path_buf().join(LOG_DIR_NAME);
        tokio::fs::create_dir_all(&log_path).await?;

        // wascc has native capabilities which are dynamic libraries (.so, .dylib, .dll)
        // and portable capabilities which are WASM modules.  Portable capabilities
        // don't fully work, and won't until the WASI spec has matured.  We load
        // logging and http serving capabilities here, by first loading the library
        // then adding the capability to the host.
        tokio::task::spawn_blocking(|| {
            info!("Loading HTTP Capability");
            let data = NativeCapability::from_file(HTTP_LIB).map_err(|e| {
                anyhow::anyhow!("Failed to read HTTP capability {}: {}", HTTP_LIB, e)
            })?;
            host::add_native_capability(data)
                .map_err(|e| {
                    anyhow::anyhow!("Failed to load HTTP capability: {}", e)
            })?;

            info!("Loading LOG Capability");
            let logdata = NativeCapability::from_file(LOG_LIB).map_err(|e| {
                anyhow::anyhow!("Failed to read LOG capability {}: {}", LOG_LIB, e)
            })?;
            host::add_native_capability(logdata)
                .map_err(|e| anyhow::anyhow!("Failed to load LOG capability: {}", e))
        })
        .await??;
        Ok(Self {
            handles: Default::default(),
            store,
            log_path,
            kubeconfig,
        })
    }
}

#[async_trait]
impl<S: ModuleStore + Send + Sync> Provider for WasccProvider<S> {
    const ARCH: &'static str = TARGET_WASM32_WASCC;

    async fn add(&self, pod: Pod) -> anyhow::Result<()> {
        // To run an Add event, we load the actor, and update the pod status 
        // to Running.  The wascc runtime takes care of starting the actor.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.
        debug!("Pod added {:?}", pod.name());

        info!("Starting containers for pod {:?}", pod.name());
        let mut modules = self.store.fetch_pod_modules(&pod).await?;
        let mut container_handles = HashMap::new();
        let client = kube::Client::from(self.kubeconfig.clone());
        for container in pod.containers() {
            let env = Self::env_vars(&container, &pod, &client).await;

            debug!("Starting container {} on thread", container.name);

            let module_data = modules
                .remove(&container.name)
                .expect("FATAL ERROR: module map not properly populated");
            let lp = self.log_path.clone();
            let (status_sender, status_recv) = watch::channel(ContainerStatus::Waiting {
                timestamp: chrono::Utc::now(),
                message: "No status has been received from the process".into(),
            });
            let http_result =
                tokio::task::spawn_blocking(move || wascc_run_http(module_data, env, &lp, status_recv))
                    .await?;
            match http_result {
                Ok(handle) => {
                    container_handles.insert(container.name.clone(), handle);
                    status_sender.broadcast(ContainerStatus::Running {
                        timestamp: chrono::Utc::now(),
                    }).expect("status should be able to send");
                }
                Err(e) => {
                    status_sender.broadcast(ContainerStatus::Terminated {
                        timestamp: chrono::Utc::now(),
                        failed: true,
                        message: format!("Error while starting container: {:?}", e),
                    }).expect("status should be able to send");
                    return Err(anyhow::anyhow!("Failed to run pod: {}", e));
                }
            }
        }
        info!(
            "All containers started for pod {:?}. Updating status",
            pod.name()
        );
        // Wrap this in a block so the write lock goes out of scope when we are done
        {
            let mut handles = self.handles.write().await;
            handles.insert(
                key_from_pod(&pod),
                PodHandle::new(container_handles, pod, client)?,
            );
        }

        Ok(())
    }

    async fn modify(&self, pod: Pod) -> anyhow::Result<()> {
        // Modify will be tricky. Not only do we need to handle legitimate modifications, but we
        // need to sift out modifications that simply alter the status. For the time being, we
        // just ignore them, which is the wrong thing to do... except that it demos better than
        // other wrong things.
        info!("Pod modified");
        info!(
            "Modified pod spec: {:#?}",
            pod.as_kube_pod().status.as_ref().unwrap()
        );
        Ok(())
    }

    async fn delete(&self, pod: Pod) -> anyhow::Result<()> {
        let mut handles = self.handles.write().await;
        if let Some(mut h) = handles.remove(&key_from_pod(&pod)) {
            h.stop().await.unwrap_or_else(|e| {
                error!(
                    "unable to stop pod {} in namespace {}: {:?}",
                    pod.name(),
                    pod.namespace(),
                    e
                );
                // Insert the pod back in to our store if we failed to delete it
                handles.insert(key_from_pod(&pod), h);
            })
        } else {
            info!(
                "unable to find pod {} in namespace {}, it was likely already deleted",
                pod.name(),
                pod.namespace()
            );
        }
        Ok(())
    }

    async fn logs(
        &self,
        namespace: String,
        pod_name: String,
        container_name: String,
    ) -> anyhow::Result<Vec<u8>> {
        let mut handles = self.handles.write().await;
        let handle = handles
            .get_mut(&pod_key(&namespace, &pod_name))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod_name.clone(),
            })?;
        let mut output = Vec::new();
        handle.output(&container_name, &mut output).await?;
        Ok(output)
    }
}

/// Run a WasCC module inside of the host, configuring it to handle HTTP requests.
///
/// This bootstraps an HTTP host, using the value of the env's `PORT` key to expose a port.
fn wascc_run_http(data: Vec<u8>, env: EnvVars, log_path: &Path, status_recv: Receiver<ContainerStatus>) -> anyhow::Result<RuntimeHandle<ActorStopper, File>> {
    let mut caps: Vec<Capability> = Vec::new();

    caps.push(Capability {
        name: HTTP_CAPABILITY,
        env: env,
    });
    wascc_run(
        data,
        &mut caps,
        log_path,
        status_recv,
    )
}

/// Capability describes a waSCC capability.
///
/// Capabilities are made available to actors through a two-part processthread:
/// - They must be registered
/// - For each actor, the capability must be configured
struct Capability {
    name: &'static str,
    env: EnvVars,
}

/// Run the given WASM data as a waSCC actor with the given public key.
///
/// The provided capabilities will be configured for this actor, but the capabilities
/// must first be loaded into the host by some other process, such as register_native_capabilities().
fn wascc_run(data: Vec<u8>, capabilities: &mut Vec<Capability>, log_path: &Path, status_recv: Receiver<ContainerStatus>) -> anyhow::Result<RuntimeHandle<ActorStopper, File>> {
    info!("sending actor to wascc host");
    let log_output = NamedTempFile::new_in(log_path)?;
    let mut logenv: HashMap<String, String> = HashMap::new();
    logenv.insert(LOG_PATH_KEY.to_string(), log_output.path().to_str().unwrap().to_owned());
    capabilities.push(Capability {
        name: LOG_CAPABILITY,
        env: logenv,
    });

    let load = Actor::from_bytes(data).map_err(|e| anyhow::anyhow!("Error loading WASM: {}", e))?;
    let pk = load.public_key();

    host::add_actor(load).map_err(|e| anyhow::anyhow!("Error adding actor: {}", e))?;
    capabilities.iter().try_for_each(|cap| {
        info!("configuring capability {}", cap.name);
        host::configure(&pk, cap.name, cap.env.clone())
            .map_err(|e| anyhow::anyhow!("Error configuring capabilities for module: {}", e))
    })?;
    info!("wascc actor executing");
    Ok(RuntimeHandle::new(ActorStopper{key: pk}, tokio::fs::File::from_std(log_output.reopen()?), status_recv))
}

#[cfg(test)]
mod test {
    use super::*;

    #[cfg(target_os = "linux")]
    const ECHO_LIB: &str = "./testdata/libecho_provider.so";
    #[cfg(target_os = "macos")]
    const ECHO_LIB: &str = "./testdata/libecho_provider.dylib";

    #[test]
    fn test_wascc_run() {

        use std::path::PathBuf;
        let data = NativeCapability::from_file(HTTP_LIB).expect("loaded http library");
        host::add_native_capability(data).expect("added http capability");
        // Open file
        let data = std::fs::read("./testdata/echo.wasm").expect("read the wasm file");

        let log_path = PathBuf::from(r"~/.krustlet");
        
        // Send into wascc_run
        wascc_run_http(
            data,
            EnvVars::new(),
            "MB4OLDIC3TCZ4Q4TGGOVAZC43VXFE2JQVRAXQMQFXUCREOOFEKOKZTY2",
           &log_path,
        )
        .expect("successfully executed a WASM");

        // Give the webserver a chance to start up.
        std::thread::sleep(std::time::Duration::from_secs(3));
        wascc_stop("MB4OLDIC3TCZ4Q4TGGOVAZC43VXFE2JQVRAXQMQFXUCREOOFEKOKZTY2")
            .expect("Removed the actor");
    }

    #[test]
    fn test_wascc_echo() {
        let data = NativeCapability::from_file(ECHO_LIB).expect("loaded echo library");
        host::add_native_capability(data).expect("added echo capability");

        let key = "MDAYLDTOZEHQFPB3CL5PAFY5UTNCW32P54XGWYX3FOM2UBRYNCP3I3BF";

        let log_path = PathBuf::from(r"~/.krustlet");
        let wasm = std::fs::read("./testdata/echo_actor_s.wasm").expect("load echo WASM");
        // TODO: use wascc_run to execute echo_actor
        wascc_run(
            wasm,
            key,
            &mut vec![Capability {
                name: "wok:echoProvider",
                env: EnvVars::new(),
            }],
            &log_path,
        )
        .expect("completed echo run")
    }
}
