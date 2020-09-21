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
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use kubelet::node::Builder;
use kubelet::pod::{key_from_pod, pod_key, Handle, Pod};
use kubelet::provider::{Provider, ProviderError};
use kubelet::store::Store;
use kubelet::volume::Ref;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::RwLock;
use wasi_runtime::Runtime;

mod states;

use states::registered::Registered;
use states::terminated::Terminated;

const TARGET_WASM32_WASI: &str = "wasm32-wasi";
const LOG_DIR_NAME: &str = "wasi-logs";
const VOLUME_DIR: &str = "volumes";

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec.
#[derive(Clone)]
pub struct WasiProvider {
    shared: SharedPodState,
}

#[derive(Clone)]
struct SharedPodState {
    handles: Arc<RwLock<HashMap<String, Handle<Runtime, wasi_runtime::HandleFactory>>>>,
    store: Arc<dyn Store + Sync + Send>,
    log_path: PathBuf,
    kubeconfig: kube::Config,
    volume_path: PathBuf,
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
            shared: SharedPodState {
                handles: Default::default(),
                store,
                log_path,
                volume_path,
                kubeconfig,
            },
        })
    }
}

struct ModuleRunContext {
    modules: HashMap<String, Vec<u8>>,
    volumes: HashMap<String, Ref>,
    status_sender: Sender<(String, kubelet::container::Status)>,
    status_recv: Receiver<(String, kubelet::container::Status)>,
}

/// State that is shared between pod state handlers.
pub struct PodState {
    key: String,
    run_context: ModuleRunContext,
    errors: usize,
    shared: SharedPodState,
}

// No cleanup state needed, we clean up when dropping PodState.
#[async_trait]
impl kubelet::state::AsyncDrop for PodState {
    async fn async_drop(self) {
        {
            let mut handles = self.shared.handles.write().await;
            handles.remove(&self.key);
        }
    }
}

#[async_trait::async_trait]
impl Provider for WasiProvider {
    type InitialState = Registered;
    type TerminatedState = Terminated;
    type PodState = PodState;

    const ARCH: &'static str = TARGET_WASM32_WASI;

    async fn node(&self, builder: &mut Builder) -> anyhow::Result<()> {
        builder.set_architecture("wasm-wasi");
        builder.add_taint("NoExecute", "kubernetes.io/arch", Self::ARCH);
        Ok(())
    }

    async fn initialize_pod_state(&self, pod: &Pod) -> anyhow::Result<Self::PodState> {
        let (tx, rx) = mpsc::channel(pod.all_containers().len());
        let run_context = ModuleRunContext {
            modules: Default::default(),
            volumes: Default::default(),
            status_sender: tx,
            status_recv: rx,
        };
        let key = key_from_pod(pod);
        Ok(PodState {
            key,
            run_context,
            errors: 0,
            shared: self.shared.clone(),
        })
    }

    async fn logs(
        &self,
        namespace: String,
        pod_name: String,
        container_name: String,
        sender: kubelet::log::Sender,
    ) -> anyhow::Result<()> {
        let mut handles = self.shared.handles.write().await;
        let handle = handles
            .get_mut(&pod_key(&namespace, &pod_name))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod_name.clone(),
            })?;
        handle.output(&container_name, sender).await
    }
}
