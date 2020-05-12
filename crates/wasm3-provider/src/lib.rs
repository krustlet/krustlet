//! A custom kubelet backend that can run [WASI](https://wasi.dev/) based workloads using wasm3
//!
//! The crate provides the [`Provider`] type which can be used
//! as a provider with [`kubelet`].
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Kubelet, config::Config};
//! use kubelet::module_store::FileModuleStore;
//! use wasm3_provider::Provider;
//!
//! async {
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!     let client = oci_distribution::Client::default();
//!     let store = FileModuleStore::new(client, &std::path::PathBuf::from(""));
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::Config::infer().await.unwrap();
//!
//!     // Instantiate the provider type
//!     let provider = Provider::new(store, &kubelet_config, kubeconfig.clone()).await.unwrap();
//!
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config);
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! };
//! ```

mod runtime;

use kubelet::handle::{key_from_pod, pod_key};
use kubelet::module_store::ModuleStore;
use kubelet::provider::ProviderError;
use kubelet::Pod;
use log::{debug, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use runtime::Runtime;

const TARGET_WASM32_WASI: &str = "wasm32-wasi";

/// Provider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec
pub struct Provider<S> {
    pods: Arc<RwLock<HashMap<String, HashMap<String, Runtime>>>>,
    store: S,
}

impl<S: ModuleStore + Send + Sync> Provider<S> {
    /// Create a new wasi provider from a module store and a kubelet config
    pub fn new(store: S) -> Self {
        Self {
            pods: Default::default(),
            store,
        }
    }
}

#[async_trait::async_trait]
impl<S: ModuleStore + Send + Sync> kubelet::Provider for Provider<S> {
    const ARCH: &'static str = TARGET_WASM32_WASI;

    async fn add(&self, pod: Pod) -> anyhow::Result<()> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.

        let pod_name = pod.name();
        let mut containers = HashMap::new();

        let mut modules = self.store.fetch_pod_modules(&pod).await?;
        info!("Starting containers for pod {:?}", pod_name);
        for container in pod.containers() {
            let module_data = modules
                .remove(&container.name)
                .expect("FATAL ERROR: module map not properly populated");

            let mut runtime = Runtime::new(module_data, 1 as u32);

            debug!("Starting container {} on thread", container.name);
            runtime.start()?;
            containers.insert(container.name.clone(), runtime);
        }
        info!(
            "All containers started for pod {:?}. Updating status",
            pod_name
        );

        // Wrap this in a block so the write lock goes out of scope when we are done
        {
            // Grab the entry while we are creating things
            let mut pods = self.pods.write().await;
            pods.insert(key_from_pod(&pod), containers);
        }

        Ok(())
    }

    async fn modify(&self, _pod: Pod) -> anyhow::Result<()> {
        unimplemented!()
    }

    async fn delete(&self, _pod: Pod) -> anyhow::Result<()> {
        unimplemented!()
    }

    async fn logs(
        &self,
        namespace: String,
        pod_name: String,
        _container_name: String,
        _sender: kubelet::LogSender,
    ) -> anyhow::Result<()> {
        let mut pods = self.pods.write().await;
        let _pod = pods
            .get_mut(&pod_key(&namespace, &pod_name))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod_name.clone(),
            })?;
        // pod.output(&container_name, sender).await
        unimplemented!()
    }
}
