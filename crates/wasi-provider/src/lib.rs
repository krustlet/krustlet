//! A custom kubelet backend that can run [WASI](https://wasi.dev/) based workloads
//!
//! The crate provides the [`WasiProvider`] type which can be used
//! as a provider with [`kubelet`].
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Kubelet, config::Config};
//! use kubelet::store::oci::FileStore;
//! use std::sync::Arc;
//! use wasi_provider::WasiProvider;
//!
//! async {
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!     let client = oci_distribution::Client::default();
//!     let store = Arc::new(FileStore::new(client, &std::path::PathBuf::from("")));
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::Config::infer().await.unwrap();
//!
//!     // Instantiate the provider type
//!     let provider = WasiProvider::new(store, &kubelet_config, kubeconfig.clone()).await.unwrap();
//!
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config).await.unwrap();
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! };
//! ```

#![deny(missing_docs)]

mod wasi_runtime;

use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use futures::stream::StreamExt;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::{api::DeleteParams, Api};
use kubelet::container::Container;
use kubelet::container::{ContainerKey, HandleMap as ContainerHandleMap, Status};
use kubelet::node::Builder;
use kubelet::pod::{key_from_pod, pod_key, Handle, Pod};
use kubelet::provider::Provider;
use kubelet::provider::ProviderError;
use kubelet::store::Store;
use kubelet::volume::Ref;
use log::{debug, error, info, trace, warn};
use tokio::sync::RwLock;
use wasi_runtime::{Runtime, WasiRuntime};

const TARGET_WASM32_WASI: &str = "wasm32-wasi";
const LOG_DIR_NAME: &str = "wasi-logs";
const VOLUME_DIR: &str = "volumes";

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec
#[derive(Clone)]
pub struct WasiProvider {
    handles: Arc<RwLock<HashMap<String, Handle<Runtime, wasi_runtime::HandleFactory>>>>,
    store: Arc<dyn Store + Sync + Send>,
    log_path: PathBuf,
    kubeconfig: kube::Config,
    volume_path: PathBuf,
}

struct ContainerTerminationResult {
    succeeded: bool,
    message: String,
}

impl WasiProvider {
    /// Create a new wasi provider from a module store and a kubelet config
    pub async fn new(
        store: Arc<dyn Store + Sync + Send>,
        config: &kubelet::config::Config,
        kubeconfig: kube::Config,
    ) -> anyhow::Result<Self> {
        let log_path = config.data_dir.join(LOG_DIR_NAME);
        let volume_path = config.data_dir.join(VOLUME_DIR);
        tokio::fs::create_dir_all(&log_path).await?;
        tokio::fs::create_dir_all(&volume_path).await?;
        Ok(Self {
            handles: Default::default(),
            store,
            log_path,
            volume_path,
            kubeconfig,
        })
    }

    fn volume_path_map(
        container: &Container,
        volumes: &HashMap<String, Ref>,
    ) -> anyhow::Result<HashMap<PathBuf, Option<PathBuf>>> {
        if let Some(volume_mounts) = container.volume_mounts().as_ref() {
            volume_mounts
                .iter()
                .map(|vm| -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
                    // Check the volume exists first
                    let vol = volumes.get(&vm.name).ok_or_else(|| {
                        anyhow::anyhow!(
                            "no volume with the name of {} found for container {}",
                            vm.name,
                            container.name()
                        )
                    })?;
                    let mut guest_path = PathBuf::from(&vm.mount_path);
                    if let Some(sub_path) = &vm.sub_path {
                        guest_path.push(sub_path);
                    }
                    // We can safely assume that this should be valid UTF-8 because it would have
                    // been validated by the k8s API
                    Ok((vol.deref().clone(), Some(guest_path)))
                })
                .collect::<anyhow::Result<HashMap<PathBuf, Option<PathBuf>>>>()
        } else {
            Ok(HashMap::default())
        }
    }

    async fn start_container(
        &self,
        container: &Container,
        pod: &Pod,
        client: &kube::client::Client,
        modules: &mut HashMap<String, Vec<u8>>,
        volumes: &HashMap<String, Ref>,
    ) -> anyhow::Result<
        kubelet::container::Handle<wasi_runtime::Runtime, wasi_runtime::HandleFactory>,
    > {
        let env = Self::env_vars(&container, pod, &client).await;
        let args = container.args().clone().unwrap_or_default();
        let module_data = modules
            .remove(container.name())
            .expect("FATAL ERROR: module map not properly populated");
        let container_volumes = Self::volume_path_map(container, volumes)?;

        let runtime = WasiRuntime::new(
            module_data,
            env,
            args,
            container_volumes,
            self.log_path.clone(),
        )
        .await?;

        debug!("Starting container {} on thread", container.name());
        let handle = runtime.start().await?;

        Ok(handle)
    }

    async fn start_app_containers(
        &self,
        pod: &Pod,
        client: &kube::client::Client,
        modules: &mut HashMap<String, Vec<u8>>,
        volumes: &HashMap<String, Ref>,
    ) -> (
        ContainerHandleMap<wasi_runtime::Runtime, wasi_runtime::HandleFactory>,
        Option<anyhow::Error>,
    ) {
        info!("Starting containers for pod {:?}", pod.name());
        let mut container_handles = HashMap::new();
        for container in pod.containers() {
            let start_result = self
                .start_container(&container, pod, client, modules, volumes)
                .await;
            match start_result {
                Ok(handle) => {
                    container_handles
                        .insert(ContainerKey::App(container.name().to_owned()), handle);
                }
                Err(e) => {
                    return (container_handles, Some(e));
                }
            }
        }
        info!(
            "All containers started for pod {:?}. Updating status",
            pod.name()
        );

        (container_handles, None)
    }

    async fn run_container_to_completion(
        &self,
        status_receiver: &mut tokio::sync::watch::Receiver<kubelet::container::Status>,
        container: &Container,
    ) -> anyhow::Result<()> {
        let result = Self::wait_for_terminated_status(status_receiver).await;
        debug!("Init container {} terminated", container.name());
        if result.succeeded {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Init container {} failed with message {}",
                container.name(),
                result.message
            ))
        }
    }

    async fn run_init_containers(
        &self,
        pod: &Pod,
        client: &kube::client::Client,
        modules: &mut HashMap<String, Vec<u8>>,
        volumes: &HashMap<String, Ref>,
    ) -> (
        ContainerHandleMap<wasi_runtime::Runtime, wasi_runtime::HandleFactory>,
        Option<anyhow::Error>,
    ) {
        info!("Running init containers for pod {:?}", pod.name());
        let mut container_handles = HashMap::new();
        for container in pod.init_containers() {
            let start_result = self
                .start_container(&container, pod, client, modules, volumes)
                .await;
            match start_result {
                Ok(handle) => {
                    let mut status_receiver = handle.status(); // TODO: ugh but borrow checker
                    container_handles
                        .insert(ContainerKey::Init(container.name().to_owned()), handle);
                    let run_result = self
                        .run_container_to_completion(&mut status_receiver, &container)
                        .await;
                    if let Err(run_error) = run_result {
                        return (container_handles, Some(run_error));
                    }
                }
                Err(e) => {
                    return (container_handles, Some(e));
                }
            }
        }
        info!("Finished running init containers for pod {:?}", pod.name());
        (container_handles, None)
    }

    async fn wait_for_terminated_status(
        status_receiver: &mut tokio::sync::watch::Receiver<kubelet::container::Status>,
    ) -> ContainerTerminationResult {
        loop {
            let status = status_receiver.next().await;
            if let Some(Status::Terminated {
                message, failed, ..
            }) = status
            {
                return ContainerTerminationResult {
                    succeeded: !failed,
                    message,
                };
            }
        }
    }
}

#[async_trait::async_trait]
impl Provider for WasiProvider {
    const ARCH: &'static str = TARGET_WASM32_WASI;

    async fn node(&self, builder: &mut Builder) -> anyhow::Result<()> {
        builder.set_architecture("wasm-wasi");
        builder.add_taint("NoExecute", "krustlet/arch", Self::ARCH);
        Ok(())
    }

    async fn add(&self, pod: Pod) -> anyhow::Result<()> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.

        let mut container_handles = ContainerHandleMap::new();

        let mut modules = self.store.fetch_pod_modules(&pod).await?;
        let client = kube::Client::new(self.kubeconfig.clone());
        let volumes = Ref::volumes_from_pod(&self.volume_path, &pod, &client).await?;

        let (init_handles, init_error) = self
            .run_init_containers(&pod, &client, &mut modules, &volumes)
            .await;
        container_handles.extend(init_handles.into_iter());

        let mut start_error = None;

        match init_error {
            None => {
                let (app_handles, app_error) = self
                    .start_app_containers(&pod, &client, &mut modules, &volumes)
                    .await;
                container_handles.extend(app_handles.into_iter());
                match app_error {
                    None => debug!("Successfully started all containers for pod {}", pod.name()),
                    Some(e) => {
                        warn!(
                            "Failed to start all containers for pod {}: {}",
                            pod.name(),
                            e
                        );
                        start_error = Some(e);
                    }
                };
            }
            Some(e) => {
                info!(
                    "Failed running init containers for pod {}: {}",
                    pod.name(),
                    e
                );
                start_error = Some(e);
            }
        };

        let pod_start_message = start_error.as_ref().map(|e| e.to_string());

        let pod_handle_key = key_from_pod(&pod);
        let pod_handle = Handle::new(
            container_handles,
            pod,
            client,
            Some(volumes),
            pod_start_message,
        )
        .await?;

        // Wrap this in a block so the write lock goes out of scope when we are done
        {
            // Grab the entry while we are creating things
            let mut handles = self.handles.write().await;
            handles.insert(pod_handle_key, pod_handle);
        }

        match start_error {
            None => Ok(()),
            Some(e) => Err(e),
        }
    }

    async fn modify(&self, pod: Pod) -> anyhow::Result<()> {
        // The only things we care about are:
        // 1. metadata.deletionTimestamp => signal all containers to stop and then mark them
        //    as terminated
        // 2. spec.containers[*].image, spec.initContainers[*].image => stop the currently
        //    running containers and start new ones?
        // 3. spec.activeDeadlineSeconds => Leaving unimplemented for now
        // TODO: Determine what the proper behavior should be if labels change
        debug!(
            "Got pod modified event for {} in namespace {}",
            pod.name(),
            pod.namespace()
        );
        trace!("Modified pod spec: {:#?}", pod.as_kube_pod());
        if let Some(_timestamp) = pod.deletion_timestamp() {
            let mut handles = self.handles.write().await;
            match handles.get_mut(&key_from_pod(&pod)) {
                Some(h) => {
                    h.stop().await?;
                    // Follow up with a delete when everything is stopped
                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        ..Default::default()
                    };
                    let pod_client: Api<KubePod> = Api::namespaced(
                        kube::client::Client::new(self.kubeconfig.clone()),
                        pod.namespace(),
                    );
                    match pod_client.delete(pod.name(), &dp).await {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e.into()),
                    }
                }
                None => {
                    // This isn't an error with the pod, so don't return an error (otherwise it will
                    // get updated in its status). This is an unlikely case to get into and means
                    // that something is likely out of sync, so just log the error
                    error!(
                        "Unable to find pod {} in namespace {} when trying to stop all containers",
                        pod.name(),
                        pod.namespace()
                    );
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
        // TODO: Implement behavior for stopping old containers and restarting when the container
        // image changes
    }

    async fn delete(&self, pod: Pod) -> anyhow::Result<()> {
        let mut handles = self.handles.write().await;
        match handles.remove(&key_from_pod(&pod)) {
            Some(_) => debug!(
                "Pod {} in namespace {} removed",
                pod.name(),
                pod.namespace()
            ),
            None => info!(
                "unable to find pod {} in namespace {}, it was likely already deleted",
                pod.name(),
                pod.namespace()
            ),
        }
        Ok(())
    }

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
