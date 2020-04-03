//! A custom kubelet backend that can run [WASI](https://wasi.dev/) based workloads
//!
//! The crate provides the [`WasiProvider`] type which can be used
//! as a provider with [`kubelet`].
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Kubelet, config::Config};
//! use kubelet::module_store::FileModuleStore;
//! use wasi_provider::WasiProvider;
//!
//! async {
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!     let client = oci_distribution::Client::default();
//!     let store = FileModuleStore::new(client, &std::path::PathBuf::from(""));
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::config::load_kube_config().await.unwrap();
//!
//!     // Instantiate the provider type
//!     let provider = WasiProvider::new(store, &kubelet_config, kubeconfig.clone()).await.unwrap();
//!     
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config);
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! };
//! ```

#![warn(missing_docs)]

mod wasi_runtime;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use kubelet::module_store::ModuleStore;
use kubelet::provider::ProviderError;
use kubelet::{Pod, Provider};
use log::{debug, error, info};
use tokio::fs::File;
use tokio::sync::RwLock;

use kubelet::handle::{PodHandle, key_from_pod, pod_key};
use wasi_runtime::{HandleStopper, WasiRuntime};

const TARGET_WASM32_WASI: &str = "wasm32-wasi";
const LOG_DIR_NAME: &str = "wasi-logs";

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec
#[derive(Clone)]
pub struct WasiProvider<S> {
    handles: Arc<RwLock<HashMap<String, PodHandle<HandleStopper, File>>>>,
    store: S,
    log_path: PathBuf,
    kubeconfig: kube::config::Configuration,
}

impl<S: ModuleStore + Send + Sync> WasiProvider<S> {
    /// Create a new wasi provider from a module store and a kubelet config
    pub async fn new(
        store: S,
        config: &kubelet::config::Config,
        kubeconfig: kube::config::Configuration,
    ) -> anyhow::Result<Self> {
        let log_path = config.data_dir.to_path_buf().join(LOG_DIR_NAME);
        tokio::fs::create_dir_all(&log_path).await?;
        Ok(Self {
            handles: Default::default(),
            store,
            log_path,
            kubeconfig,
        })
    }
}

#[async_trait::async_trait]
impl<S: ModuleStore + Send + Sync> Provider for WasiProvider<S> {
    const ARCH: &'static str = TARGET_WASM32_WASI;

    async fn add(&self, pod: Pod) -> anyhow::Result<()> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.

        // TODO: Implement this for real.
        //
        // What it should do:
        // - for each volume
        //   - set up the volume map
        // - for each init container:
        //   - set up the runtime
        //   - mount any volumes (preopen)
        //   - run it to completion
        //   - bail with an error if it fails
        // - for each container and ephemeral_container
        //   - set up the runtime
        //   - mount any volumes (popen)
        //   - run it to completion
        //   - bail if it errors
        let pod_name = pod.name();
        let mut container_handles = HashMap::new();

        let mut modules = self.store.fetch_pod_modules(&pod).await?;
        let client = kube::Client::from(self.kubeconfig.clone());
        info!("Starting containers for pod {:?}", pod_name);
        for container in pod.containers() {
            let env = Self::env_vars(&container, &pod, &client).await;
            let module_data = modules
                .remove(&container.name)
                .expect("FATAL ERROR: module map not properly populated");

            let runtime = WasiRuntime::new(
                module_data,
                env,
                Vec::default(),
                HashMap::default(),
                self.log_path.clone(),
            )
            .await?;

            debug!("Starting container {} on thread", container.name);
            let handle = runtime.start().await?;
            container_handles.insert(container.name.clone(), handle);
        }
        info!(
            "All containers started for pod {:?}. Updating status",
            pod_name
        );

        // Wrap this in a block so the write lock goes out of scope when we are done
        {
            // Grab the entry while we are creating things
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
        // There is currently no way to stop a long running instance, so we are
        // SOL here until there is support for it. See
        // https://github.com/bytecodealliance/wasmtime/issues/860 for more
        // information. For now, just delete the handle from the map
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

/// Generates a unique human readable key for storing a handle to a pod
fn key_from_pod(pod: &Pod) -> String {
    pod_key(pod.namespace(), pod.name())
}

fn pod_key<N: AsRef<str>, T: AsRef<str>>(namespace: N, pod_name: T) -> String {
    format!("{}:{}", namespace.as_ref(), pod_name.as_ref())
}
