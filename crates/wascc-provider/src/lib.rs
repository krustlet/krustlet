//! A custom kubelet backend that can run [waSCC](https://wascc.dev/) based workloads
//!
//! The crate provides the [`WasccProvider`] type which can be used
//! as a provider with [`kubelet`].
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Kubelet, config::Config};
//! use kubelet::store::oci::FileStore;
//! use std::sync::Arc;
//! use wascc_provider::WasccProvider;
//!
//! async fn start() {
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!     let client = oci_distribution::Client::default();
//!     let store = Arc::new(FileStore::new(client, &std::path::PathBuf::from("")));
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::Config::infer().await.unwrap();
//!
//!     // Instantiate the provider type
//!     let provider = WasccProvider::new(store, &kubelet_config, kubeconfig.clone()).await.unwrap();
//!
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config).await.unwrap();
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! }
//! ```

#![deny(missing_docs)]

use async_trait::async_trait;
use kubelet::container::{Handle as ContainerHandle, Status as ContainerStatus};
use kubelet::handle::StopHandler;
use kubelet::node::Builder;
use kubelet::pod::Phase;
use kubelet::pod::{pod_key, Handle};
use kubelet::provider::Provider;
use kubelet::provider::ProviderError;
use kubelet::store::Store;

use kubelet::volume::Ref;
use log::{debug, info};
use tempfile::NamedTempFile;
use tokio::sync::watch::Receiver;
use tokio::sync::RwLock;
use wascc_fs::FileSystemProvider;
use wascc_host::{Actor, NativeCapability, WasccHost};
use wascc_httpsrv::HttpServerProvider;
use wascc_logging::{LoggingProvider, LOG_PATH_KEY};

extern crate rand;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;

mod states;
use states::registered::Registered;

/// The architecture that the pod targets.
const TARGET_WASM32_WASCC: &str = "wasm32-wascc";

/// The name of the Filesystem capability.
const FS_CAPABILITY: &str = "wascc:blobstore";

/// The name of the HTTP capability.
const HTTP_CAPABILITY: &str = "wascc:http_server";

/// The name of the Logging capability.
const LOG_CAPABILITY: &str = "wascc:logging";

/// The root directory of waSCC logs.
const LOG_DIR_NAME: &str = "wascc-logs";

/// The key used to define the root directory of the Filesystem capability.
const FS_CONFIG_ROOTDIR: &str = "ROOT";

/// The root directory of waSCC volumes.
const VOLUME_DIR: &str = "volumes";

/// Kubernetes' view of environment variables is an unordered map of string to string.
type EnvVars = std::collections::HashMap<String, String>;

/// A [kubelet::handle::Handle] implementation for a wascc actor
pub struct ActorHandle {
    /// The public key of the wascc Actor that will be stopped
    pub key: String,
    host: Arc<Mutex<WasccHost>>,
    volumes: Vec<VolumeBinding>,
}

#[async_trait::async_trait]
impl StopHandler for ActorHandle {
    async fn stop(&mut self) -> anyhow::Result<()> {
        debug!("stopping wascc instance {}", self.key);
        let host = self.host.clone();
        let key = self.key.clone();
        let volumes: Vec<VolumeBinding> = self.volumes.drain(0..).collect();
        tokio::task::spawn_blocking(move || {
            let lock = host.lock().unwrap();
            lock.remove_actor(&key)
                .map_err(|e| anyhow::anyhow!("unable to remove actor: {:?}", e))?;
            for volume in volumes.into_iter() {
                lock.remove_native_capability(FS_CAPABILITY, Some(volume.name))
                    .map_err(|e| anyhow::anyhow!("unable to remove volume capability: {:?}", e))?;
            }
            Ok(())
        })
        .await?
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
pub struct WasccProvider {
    client: kube::Client,
    handles: Arc<RwLock<HashMap<String, Handle<ActorHandle, LogHandleFactory>>>>,
    run_contexts: Arc<RwLock<HashMap<String, ModuleRunContext>>>,
    store: Arc<dyn Store + Sync + Send>,
    volume_path: PathBuf,
    log_path: PathBuf,
    host: Arc<Mutex<WasccHost>>,
    port_map: Arc<TokioMutex<HashMap<i32, String>>>,
}

impl WasccProvider {
    /// Returns a new wasCC provider configured to use the proper data directory
    /// (including creating it if necessary)
    pub async fn new(
        store: Arc<dyn Store + Sync + Send>,
        config: &kubelet::config::Config,
        kubeconfig: kube::Config,
    ) -> anyhow::Result<Self> {
        let client = kube::Client::new(kubeconfig);
        let host = Arc::new(Mutex::new(WasccHost::new()));
        let log_path = config.data_dir.join(LOG_DIR_NAME);
        let volume_path = config.data_dir.join(VOLUME_DIR);
        let port_map = Arc::new(TokioMutex::new(HashMap::<i32, String>::new()));
        tokio::fs::create_dir_all(&log_path).await?;
        tokio::fs::create_dir_all(&volume_path).await?;

        // wascc has native and portable capabilities.
        //
        // Native capabilities are either dynamic libraries (.so, .dylib, .dll)
        // or statically linked Rust libaries. If the native capabilty is a dynamic
        // library it must be loaded and configured through [`NativeCapability::from_file`].
        // If it is a statically linked libary it can be configured through
        // [`NativeCapability::from_instance`].
        //
        // Portable capabilities are WASM modules.  Portable capabilities
        // don't fully work, and won't until the WASI spec has matured.
        //
        // Here we are using the native capabilties as statically linked libraries that will
        // be compiled into the wascc-provider binary.
        let cloned_host = host.clone();
        tokio::task::spawn_blocking(move || {
            info!("Loading HTTP capability");
            let http_provider = HttpServerProvider::new();
            let data = NativeCapability::from_instance(http_provider, None)
                .map_err(|e| anyhow::anyhow!("Failed to instantiate HTTP capability: {}", e))?;

            cloned_host
                .lock()
                .unwrap()
                .add_native_capability(data)
                .map_err(|e| anyhow::anyhow!("Failed to add HTTP capability: {}", e))?;

            info!("Loading log capability");
            let logging_provider = LoggingProvider::new();
            let logging_capability = NativeCapability::from_instance(logging_provider, None)
                .map_err(|e| anyhow::anyhow!("Failed to instantiate log capability: {}", e))?;
            cloned_host
                .lock()
                .unwrap()
                .add_native_capability(logging_capability)
                .map_err(|e| anyhow::anyhow!("Failed to add log capability: {}", e))
        })
        .await??;
        Ok(Self {
            client,
            handles: Default::default(),
            run_contexts: Default::default(),
            store,
            volume_path,
            log_path,
            host,
            port_map,
        })
    }
}

struct ModuleRunContext {
    modules: HashMap<String, Vec<u8>>,
    volumes: HashMap<String, Ref>,
}

fn make_status(phase: Phase, reason: &str) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::json!(
       {
           "metadata": {
               "resourceVersion": "",
           },
           "status": {
               "phase": phase,
               "reason": reason,
               "containerStatuses": Vec::<()>::new(),
               "initContainerStatuses": Vec::<()>::new(),
           }
       }
    ))
}

/// State that is shared between pod state handlers.
pub struct PodState {
    run_context: ModuleRunContext,
    store: Arc<dyn Store + Sync + Send>,
    client: kube::Client,
    volume_path: std::path::PathBuf,
    errors: usize,
    port_map: Arc<TokioMutex<HashMap<i32, String>>>,
    handle: Option<Handle<ActorHandle, LogHandleFactory>>,
    log_path: PathBuf,
    host: Arc<Mutex<WasccHost>>,
}

#[async_trait]
impl Provider for WasccProvider {
    type InitialState = Registered;
    type PodState = PodState;

    const ARCH: &'static str = TARGET_WASM32_WASCC;

    async fn node(&self, builder: &mut Builder) -> anyhow::Result<()> {
        builder.set_architecture("wasm-wasi");
        builder.add_taint("NoExecute", "krustlet/arch", Self::ARCH);
        Ok(())
    }

    async fn initialize_pod_state(&self) -> anyhow::Result<Self::PodState> {
        let run_context = ModuleRunContext {
            modules: Default::default(),
            volumes: Default::default(),
        };

        Ok(PodState {
            run_context,
            store: Arc::clone(&self.store),
            client: self.client.clone(),
            volume_path: self.volume_path.clone(),
            errors: 0,
            port_map: Arc::clone(&self.port_map),
            handle: None,
            log_path: self.log_path.clone(),
            host: Arc::clone(&self.host),
        })
    }

    // async fn modify(&self, pod: Pod) {
    //     let pod_handle_key = key_from_pod(&pod);
    //     // The only things we care about are:
    //     // 1. metadata.deletionTimestamp => signal all containers to stop and then mark them
    //     //    as terminated
    //     // 2. spec.containers[*].image, spec.initContainers[*].image => stop the currently
    //     //    running containers and start new ones?
    //     // 3. spec.activeDeadlineSeconds => Leaving unimplemented for now
    //     // TODO: Determine what the proper behavior should be if labels change
    //     let pod_name = pod.name().to_owned();
    //     let pod_namespace = pod.namespace().to_owned();
    //     debug!(
    //         "Got pod modified event for {} in namespace {}",
    //         pod_name, pod_namespace
    //     );
    //     trace!("Modified pod spec: {:#?}", pod.as_kube_pod());
    //     if let Some(_timestamp) = pod.deletion_timestamp() {
    //         debug!(
    //             "Found delete timestamp for pod {} in namespace {}. Stopping running actors",
    //             pod_name, pod_namespace
    //         );
    //         let mut handles = self.handles.write().await;
    //         match handles.get_mut(&key_from_pod(&pod)) {
    //             Some(h) => {
    //                 h.stop().await.unwrap();

    //                 debug!(
    //                     "All actors stopped for pod {} in namespace {}, updating status",
    //                     pod_name, pod_namespace
    //                 );
    //                 // Having to do this here isn't my favorite thing, but we need to update the
    //                 // status of the container so it can be deleted. We will probably need to have
    //                 // some sort of provider that can send a message about status to the Kube API
    //                 let now = chrono::Utc::now();
    //                 let terminated = ContainerStatus::Terminated {
    //                     timestamp: now,
    //                     message: "Pod stopped".to_owned(),
    //                     failed: false,
    //                 };

    //                 let container_statuses: Vec<KubeContainerStatus> = pod
    //                     .into_kube_pod()
    //                     .spec
    //                     .unwrap_or_default()
    //                     .containers
    //                     .into_iter()
    //                     .map(|c| terminated.to_kubernetes(c.name))
    //                     .collect();

    //                 let json_status = serde_json::json!(
    //                     {
    //                         "metadata": {
    //                             "resourceVersion": "",
    //                         },
    //                         "status": {
    //                             "message": "Pod stopped",
    //                             "phase": Phase::Succeeded,
    //                             "containerStatuses": container_statuses,
    //                         }
    //                     }
    //                 );
    //                 update_status(self.client.clone(), &pod_namespace, &pod_name, &json_status).await.unwrap();

    //                 let pod_client: Api<KubePod> = Api::namespaced(self.client.clone(), &pod_namespace);
    //                 let dp = DeleteParams {
    //                     grace_period_seconds: Some(0),
    //                     ..Default::default()
    //                 };
    //                 pod_client.delete(&pod_name, &dp).await.unwrap();
    //             }
    //             None => {
    //                 // This isn't an error with the pod, so don't return an error (otherwise it will
    //                 // get updated in its status). This is an unlikely case to get into and means
    //                 // that something is likely out of sync, so just log the error
    //                 error!(
    //                     "Unable to find pod {} in namespace {} when trying to stop all containers",
    //                     pod_name, pod_namespace
    //                 );
    //             }
    //         }
    //     } else {
    //     };
    //     // TODO: Implement behavior for stopping old containers and restarting when the container
    //     // image changes
    // }

    async fn logs(
        &self,
        namespace: String,
        pod_name: String,
        container_name: String,
        sender: kubelet::log::Sender,
    ) -> anyhow::Result<()> {
        let mut handles = self.handles.write().await;
        let handle = handles
            .get_mut(&pod_key(&namespace, &pod_name))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod_name.clone(),
            })?;
        handle.output(&container_name, sender).await
    }
}

struct VolumeBinding {
    name: String,
    host_path: PathBuf,
}

/// Run a WasCC module inside of the host, configuring it to handle HTTP requests.
///
/// This bootstraps an HTTP host, using the value of the env's `PORT` key to expose a port.
fn wascc_run_http(
    host: Arc<Mutex<WasccHost>>,
    data: Vec<u8>,
    mut env: EnvVars,
    volumes: Vec<VolumeBinding>,
    log_path: &Path,
    status_recv: Receiver<ContainerStatus>,
    port_assigned: i32,
) -> anyhow::Result<ContainerHandle<ActorHandle, LogHandleFactory>> {
    let mut caps: Vec<Capability> = Vec::new();

    env.insert("PORT".to_string(), port_assigned.to_string());
    caps.push(Capability {
        name: HTTP_CAPABILITY,
        binding: None,
        env,
    });
    wascc_run(host, data, &mut caps, volumes, log_path, status_recv)
}

/// Capability describes a waSCC capability.
///
/// Capabilities are made available to actors through a two-part processthread:
/// - They must be registered
/// - For each actor, the capability must be configured
struct Capability {
    name: &'static str,
    binding: Option<String>,
    env: EnvVars,
}

/// Holds our tempfile handle.
struct LogHandleFactory {
    temp: NamedTempFile,
}

impl kubelet::log::HandleFactory<tokio::fs::File> for LogHandleFactory {
    /// Creates `tokio::fs::File` on demand for log reading.
    fn new_handle(&self) -> tokio::fs::File {
        tokio::fs::File::from_std(self.temp.reopen().unwrap())
    }
}

/// Run the given WASM data as a waSCC actor with the given public key.
///
/// The provided capabilities will be configured for this actor, but the capabilities
/// must first be loaded into the host by some other process, such as register_native_capabilities().
fn wascc_run(
    host: Arc<Mutex<WasccHost>>,
    data: Vec<u8>,
    capabilities: &mut Vec<Capability>,
    volumes: Vec<VolumeBinding>,
    log_path: &Path,
    status_recv: Receiver<ContainerStatus>,
) -> anyhow::Result<ContainerHandle<ActorHandle, LogHandleFactory>> {
    info!("sending actor to wascc host");
    let log_output = NamedTempFile::new_in(log_path)?;
    let mut logenv: HashMap<String, String> = HashMap::new();
    logenv.insert(
        LOG_PATH_KEY.to_string(),
        log_output.path().to_str().unwrap().to_owned(),
    );
    capabilities.push(Capability {
        name: LOG_CAPABILITY,
        binding: None,
        env: logenv,
    });

    let load = Actor::from_bytes(data).map_err(|e| anyhow::anyhow!("Error loading WASM: {}", e))?;
    let pk = load.public_key();

    if load.capabilities().contains(&FS_CAPABILITY.to_owned()) {
        for vol in &volumes {
            info!(
                "Loading File System capability for volume name: '{}' host_path: '{}'",
                vol.name,
                vol.host_path.display()
            );
            let mut fsenv: HashMap<String, String> = HashMap::new();
            fsenv.insert(
                FS_CONFIG_ROOTDIR.to_owned(),
                vol.host_path.as_path().to_str().unwrap().to_owned(),
            );
            let fs_provider = FileSystemProvider::new();
            let fs_capability =
                NativeCapability::from_instance(fs_provider, Some(vol.name.clone())).map_err(
                    |e| anyhow::anyhow!("Failed to instantiate File System capability: {}", e),
                )?;
            host.lock()
                .unwrap()
                .add_native_capability(fs_capability)
                .map_err(|e| anyhow::anyhow!("Failed to add File System capability: {}", e))?;
            capabilities.push(Capability {
                name: FS_CAPABILITY,
                binding: Some(vol.name.clone()),
                env: fsenv,
            });
        }
    }

    host.lock()
        .unwrap()
        .add_actor(load)
        .map_err(|e| anyhow::anyhow!("Error adding actor: {}", e))?;
    capabilities.iter().try_for_each(|cap| {
        info!("configuring capability {}", cap.name);
        host.lock()
            .unwrap()
            .bind_actor(&pk, cap.name, cap.binding.clone(), cap.env.clone())
            .map_err(|e| anyhow::anyhow!("Error configuring capabilities for module: {}", e))
    })?;

    let log_handle_factory = LogHandleFactory { temp: log_output };

    info!("wascc actor executing");
    Ok(ContainerHandle::new(
        ActorHandle {
            host,
            key: pk,
            volumes,
        },
        log_handle_factory,
        status_recv,
    ))
}
