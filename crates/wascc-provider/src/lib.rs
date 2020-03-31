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
use kube::client::APIClient;
use kubelet::module_store::ModuleStore;
use kubelet::provider::NotImplementedError;
use kubelet::status::{ContainerStatus, Status};
use kubelet::{Pod, Provider};
use kubelet::PodHandle;
use log::{debug, info, warn};
use wascc_host::{host, Actor, NativeCapability};
use tokio::sync::RwLock;
use tokio::fs::File;

use wascc_logging::{LOG_PATH_KEY};

use std::collections::HashMap;
use std::path::{PathBuf, Path};
use std::sync::Arc;

const ACTOR_PUBLIC_KEY: &str = "deislabs.io/wascc-action-key";
const TARGET_WASM32_WASCC: &str = "wasm32-wascc";

/// The name of the HTTP capability.
const HTTP_CAPABILITY: &str = "wascc:http_server";
const LOG_CAPABILITY: &str = "wascc:logging";

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

/// WasccProvider provides a Kubelet runtime implementation that executes WASM binaries.
///
/// Currently, this runtime uses WASCC as a host, loading the primary container as an actor.
/// TODO: In the future, we will look at loading capabilities using the "sidecar" metaphor
/// from Kubernetes.
#[derive(Clone)]
pub struct WasccProvider<S> {
    handles: Arc<RwLock<HashMap<String, PodHandle<File>>>>,
    store: S,
    log_path: PathBuf,
}

impl<S: ModuleStore + Send + Sync> WasccProvider<S> {
    /// Returns a new wasCC provider configured to use the proper data directory
    /// (including creating it if necessary)
    pub async fn new(store: S, config: &kubelet::config::Config) -> anyhow::Result<Self> {
        let log_path = config.data_dir.to_path_buf().join(LOG_DIR_NAME);
        tokio::fs::create_dir_all(&log_path).await?;

        tokio::task::spawn_blocking(|| {
            warn!("Loading HTTP Capability");
            let data = NativeCapability::from_file(HTTP_LIB).map_err(|e| {
                anyhow::anyhow!("Failed to read HTTP capability {}: {}", HTTP_LIB, e)
            })?;
            host::add_native_capability(data)
                .map_err(|e| {
                    anyhow::anyhow!("Failed to load HTTP capability: {}", e)
            })?;

            warn!("Loading LOG Capability");
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
        })
    }
}

#[async_trait]
impl<S: ModuleStore + Send + Sync> Provider for WasccProvider<S> {
    fn arch(&self) -> String {
        TARGET_WASM32_WASCC.to_string()
    }

    fn can_schedule(&self, pod: &Pod) -> bool {
        // If there is a node selector and it has arch set to wasm32-wascc, we can
        // schedule it.
        pod.node_selector()
            .and_then(|i| {
                i.get("beta.kubernetes.io/arch")
                    .map(|v| v.eq(&TARGET_WASM32_WASCC))
            })
            .unwrap_or(false)
    }

    async fn add(&self, pod: Pod, client: APIClient) -> anyhow::Result<()> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.
        debug!("Pod added {:?}", pod.name());
        // This would lock us into one wascc actor per pod. I don't know if
        // that is a good thing. Other containers would then be limited
        // to acting as components... which largely follows the sidecar
        // pattern.
        //
        // Another possibility is to embed the key in the image reference
        // (image/foo.wasm@ed25519:PUBKEY). That might work best, but it is
        // not terribly useable.
        //
        // A really icky one would be to just require the pubkey in the env
        // vars and suck it out of there. But that violates the intention
        // of env vars, which is to communicate _into_ the runtime, not to
        // configure the runtime.
        // TODO: This isn't the pub-key you're looking for, remove annotations from the pod spec
        // pull it from the actor
        let pub_key = pod.get_annotation(ACTOR_PUBLIC_KEY).unwrap_or_default();
        debug!("{:?}", pub_key);

        // TODO: Implement this for real.
        //
        // What it should do:
        // - for each volume
        //   - set up the volume map
        // - for each init container:
        //   - set up the runtime
        //   - mount any volumes (popen)
        //   - run it to completion
        //   - bail with an error if it fails
        // - for each container and ephemeral_container
        //   - set up the runtime
        //   - mount any volumes (popen)
        //   - run it to completion
        //   - bail if it errors

        info!("Starting containers for pod {:?}", pod.name());
        let mut modules = self.store.fetch_pod_modules(&pod).await?;
        for container in pod.containers() {
            let env = self.env_vars(client.clone(), &container, &pod).await;

            debug!("Starting container {} on thread", container.name);
            let pub_key = pub_key.to_owned();

            let module_data = modules
                .remove(&container.name)
                .expect("FATAL ERROR: module map not properly populated");
            let lp = self.log_path.clone();
            let http_result =
                tokio::task::spawn_blocking(move || wascc_run_http(module_data, env, &pub_key, &lp))
                    .await?;
            match http_result {
                Ok(_) => {
                    let mut container_statuses = HashMap::new();
                    container_statuses.insert(
                        container.name.clone(),
                        ContainerStatus::Running {
                            timestamp: chrono::Utc::now(),
                        },
                    );
                    let status = Status {
                        container_statuses,
                        ..Default::default()
                    };
                    pod.patch_status(client.clone(), status).await;
                }
                Err(e) => {
                    let mut container_statuses = HashMap::new();
                    container_statuses.insert(
                        container.name.clone(),
                        ContainerStatus::Terminated {
                            timestamp: chrono::Utc::now(),
                            failed: true,
                            message: "Error while starting container".to_string(),
                        },
                    );
                    let status = Status {
                        container_statuses,
                        ..Default::default()
                    };
                    pod.patch_status(client, status).await;
                    return Err(anyhow::anyhow!("Failed to run pod: {}", e));
                }
            }
        }
        info!(
            "All containers started for pod {:?}. Updating status",
            pod.name()
        );
        Ok(())
    }

    async fn modify(&self, pod: Pod, _client: APIClient) -> anyhow::Result<()> {
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

    async fn delete(&self, pod: Pod, _client: APIClient) -> anyhow::Result<()> {
        // TODO: this isn't the correct public key
        let pub_key = pod
            .annotations()
            .get(ACTOR_PUBLIC_KEY)
            .map(String::as_str)
            .unwrap_or_default();
        wascc_stop(&pub_key).map_err(|e| anyhow::anyhow!("Failed to stop wascc actor: {}", e))
    }

    async fn logs(
        &self,
        _namespace: String,
        _pod_name: String,
        _container_name: String,
    ) -> anyhow::Result<Vec<u8>> {
        Err(NotImplementedError.into())
    }
}

/// Run a WasCC module inside of the host, configuring it to handle HTTP requests.
///
/// This bootstraps an HTTP host, using the value of the env's `PORT` key to expose a port.
fn wascc_run_http(data: Vec<u8>, env: EnvVars, key: &str, log_path: &Path) -> anyhow::Result<()> {
    let mut caps: Vec<Capability> = Vec::new();

    caps.push(Capability {
        name: HTTP_CAPABILITY,
        env: env,
    });
    wascc_run(
        data,
        key,
        &mut caps,
        log_path,
    )
}

/// Stop a running waSCC actor.
fn wascc_stop(key: &str) -> anyhow::Result<(), wascc_host::errors::Error> {
    host::remove_actor(key)
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
fn wascc_run(data: Vec<u8>, key: &str, capabilities: &mut Vec<Capability>, log_path: &Path) -> anyhow::Result<()> {
    info!("wascc run");
    let load = Actor::from_bytes(data).map_err(|e| anyhow::anyhow!("Error loading WASM: {}", e))?;
    let pk = load.public_key();

    let mut logenv: HashMap<String, String> = HashMap::new();
    let actor_path = log_path.join(pk.clone());
    std::fs::create_dir_all(&actor_path).map_err(|e| anyhow::anyhow!("error creating directory: {}", e))?;
    let actor_log_path = log_path.join(pk.clone()).join("log.txt");
    let _ = std::fs::File::create(&actor_log_path).map_err(|e| anyhow::anyhow!("error creating directory: {}", e))?;
    logenv.insert(LOG_PATH_KEY.to_string(), actor_log_path.to_str().unwrap().to_owned());
    capabilities.push(Capability {
        name: LOG_CAPABILITY,
        env: logenv,
    });
    host::add_actor(load).map_err(|e| anyhow::anyhow!("Error adding actor: {}", e))?;

    capabilities.iter().try_for_each(|cap| {
        info!("configuring capability {}", cap.name);
        host::configure(&pk, cap.name, cap.env.clone())
            .map_err(|e| anyhow::anyhow!("Error configuring capabilities for module: {}", e))
    })?;
    info!("Instance executing");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use k8s_openapi::api::core::v1::Pod as KubePod;
    use k8s_openapi::api::core::v1::PodSpec;
    use oci_distribution::Reference;

    pub struct TestStore {
        modules: HashMap<Reference, Vec<u8>>,
    }

    impl TestStore {
        fn new(modules: HashMap<Reference, Vec<u8>>) -> Self {
            Self { modules }
        }
    }

    #[async_trait]
    impl ModuleStore for TestStore {
        async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
            self.modules
                .get(image_ref)
                .cloned()
                .ok_or(anyhow::anyhow!("Failed to find module for reference"))
        }
    }

    #[cfg(target_os = "linux")]
    const ECHO_LIB: &str = "./testdata/libecho_provider.so";
    #[cfg(target_os = "macos")]
    const ECHO_LIB: &str = "./testdata/libecho_provider.dylib";

    #[test]
    fn test_wascc_run() {

        use std::path::PathBuf;
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

    #[tokio::test]
    async fn test_can_schedule() {
        let store = TestStore::new(Default::default());

        let wr = WasccProvider::new(store, &Default::default())
            .await
            .unwrap();
        let mock = Default::default();
        assert!(!wr.can_schedule(&mock));

        let mut selector = std::collections::BTreeMap::new();
        selector.insert(
            "beta.kubernetes.io/arch".to_string(),
            "wasm32-wascc".to_string(),
        );
        let mut mock: KubePod = mock.into();
        mock.spec = Some(PodSpec {
            node_selector: Some(selector.clone()),
            ..Default::default()
        });
        let mock = Pod::new(mock);
        assert!(wr.can_schedule(&mock));
        selector.insert("beta.kubernetes.io/arch".to_string(), "amd64".to_string());
        let mut mock: KubePod = mock.into();
        mock.spec = Some(PodSpec {
            node_selector: Some(selector),
            ..Default::default()
        });
        let mock = Pod::new(mock);
        assert!(!wr.can_schedule(&mock));
    }
}
