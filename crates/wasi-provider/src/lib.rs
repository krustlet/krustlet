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
use kubelet::backoff::{BackoffStrategy, ExponentialBackoffStrategy};
use kubelet::node::Builder;
use kubelet::pod::{Handle, Pod, PodKey};
use kubelet::provider::{Provider, ProviderError};
use kubelet::state::common::registered::Registered;
use kubelet::state::common::terminated::Terminated;
use kubelet::state::common::{
    BackoffSequence, GenericPodState, GenericProvider, GenericProviderState, ThresholdTrigger,
};
use kubelet::state::prelude::SharedState;
use kubelet::store::Store;
use kubelet::volume::Ref;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::RwLock;
use wasi_runtime::Runtime;

mod states;
use kubelet::pod::state::prelude::ResourceState;
use states::pod::registered::Registered;
use states::pod::terminated::Terminated;

const TARGET_WASM32_WASI: &str = "wasm32-wasi";
const LOG_DIR_NAME: &str = "wasi-logs";
const VOLUME_DIR: &str = "volumes";

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec.
#[derive(Clone)]
pub struct WasiProvider {
    shared: ProviderState,
}

/// Provider-level state shared between all pods
#[derive(Clone)]
pub struct ProviderState {
    handles: Arc<RwLock<HashMap<PodKey, Handle<Runtime, wasi_runtime::HandleFactory>>>>,
    store: Arc<dyn Store + Sync + Send>,
    log_path: PathBuf,
    kubeconfig: kube::Config,
    volume_path: PathBuf,
}

#[async_trait]
impl GenericProviderState for ProviderState {
    fn client(&self) -> kube::client::Client {
        kube::Client::new(self.kubeconfig.clone())
    }
    fn store(&self) -> std::sync::Arc<(dyn Store + Send + Sync + 'static)> {
        self.store.clone()
    }
    fn volume_path(&self) -> PathBuf {
        self.volume_path.clone()
    }
    async fn stop(&self, pod: &Pod) -> anyhow::Result<()> {
        let key = PodKey::from(pod);
        let mut handle_writer = self.handles.write().await;
        if let Some(handle) = handle_writer.get_mut(&key) {
            handle.stop().await
        } else {
            Ok(())
        }
    }
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
            shared: ProviderState {
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
    key: PodKey,
    run_context: ModuleRunContext,
    errors: usize,
    image_pull_backoff_strategy: ExponentialBackoffStrategy,
    crash_loop_backoff_strategy: ExponentialBackoffStrategy,
}

impl ResourceState for PodState {
    type Manifest = Pod;
}

// No cleanup state needed, we clean up when dropping PodState.
#[async_trait]
impl kubelet::state::AsyncDrop for PodState {
    type ProviderState = ProviderState;
    async fn async_drop(self, provider_state: &mut ProviderState) {
        {
            let mut handles = provider_state.handles.write().await;
            handles.remove(&self.key);
        }
    }
}

#[async_trait]
impl GenericPodState for PodState {
    fn set_modules(&mut self, modules: HashMap<String, Vec<u8>>) {
        self.run_context.modules = modules;
    }
    fn set_volumes(&mut self, volumes: HashMap<String, kubelet::volume::Ref>) {
        self.run_context.volumes = volumes;
    }
    async fn backoff(&mut self, sequence: BackoffSequence) {
        let backoff_strategy = match sequence {
            BackoffSequence::ImagePull => &mut self.image_pull_backoff_strategy,
            BackoffSequence::CrashLoop => &mut self.crash_loop_backoff_strategy,
        };
        backoff_strategy.wait().await;
    }
    fn reset_backoff(&mut self, sequence: BackoffSequence) {
        let backoff_strategy = match sequence {
            BackoffSequence::ImagePull => &mut self.image_pull_backoff_strategy,
            BackoffSequence::CrashLoop => &mut self.crash_loop_backoff_strategy,
        };
        backoff_strategy.reset();
    }
    fn record_error(&mut self) -> ThresholdTrigger {
        self.errors += 1;
        if self.errors > 3 {
            self.errors = 0;
            ThresholdTrigger::Triggered
        } else {
            ThresholdTrigger::Untriggered
        }
    }
}

#[async_trait::async_trait]
impl Provider for WasiProvider {
    type InitialState = Registered<Self>;
    type TerminatedState = Terminated<Self>;
    type ProviderState = ProviderState;
    type PodState = PodState;

    const ARCH: &'static str = TARGET_WASM32_WASI;

    fn provider_state(&self) -> SharedState<ProviderState> {
        SharedState::new(self.shared.clone())
    }

    async fn node(&self, builder: &mut Builder) -> anyhow::Result<()> {
        builder.set_architecture("wasm-wasi");
        builder.add_taint("NoSchedule", "kubernetes.io/arch", Self::ARCH);
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
        let key = PodKey::from(pod);
        Ok(PodState {
            key,
            run_context,
            errors: 0,
            image_pull_backoff_strategy: ExponentialBackoffStrategy::default(),
            crash_loop_backoff_strategy: ExponentialBackoffStrategy::default(),
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
            .get_mut(&PodKey::new(&namespace, &pod_name))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod_name.clone(),
            })?;
        handle.output(&container_name, sender).await
    }
}

impl GenericProvider for WasiProvider {
    type ProviderState = ProviderState;
    type PodState = PodState;
    type RunState = crate::states::initializing::Initializing;

    fn validate_pod_runnable(_pod: &Pod) -> anyhow::Result<()> {
        Ok(())
    }

    fn validate_container_runnable(
        container: &kubelet::container::Container,
    ) -> anyhow::Result<()> {
        if let Some(image) = container.image()? {
            if image.whole().starts_with("k8s.gcr.io/kube-proxy") {
                return Err(anyhow::anyhow!("Cannot run kube-proxy"));
            }
        }
        Ok(())
    }
}
