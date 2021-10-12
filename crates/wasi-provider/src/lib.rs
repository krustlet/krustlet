//! A custom kubelet backend that can run [WASI](https://wasi.dev/) based workloads
//!
//! The crate provides the [`WasiProvider`] type which can be used
//! as a provider with [`kubelet`].
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Kubelet, config::Config};
//! use kubelet::resources::DeviceManager;
//! use kubelet::store::oci::FileStore;
//! use std::convert::TryFrom;
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
//!     let plugin_registry = Arc::new(Default::default());
//!     let device_plugin_manager = Arc::new(DeviceManager::new_with_default_path(kube::Client::try_from(kubeconfig.clone()).unwrap(), &kubelet_config.node_name));
//!
//!     // Instantiate the provider type
//!     let provider = WasiProvider::new(store, &kubelet_config, kubeconfig.clone(), plugin_registry, device_plugin_manager).await.unwrap();
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
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use kubelet::node::Builder;
use kubelet::plugin_watcher::PluginRegistry;
use kubelet::pod::state::prelude::SharedState;
use kubelet::pod::{Handle, Pod, PodKey};
use kubelet::provider::{
    DevicePluginSupport, PluginSupport, Provider, ProviderError, VolumeSupport,
};
use kubelet::resources::DeviceManager;
use kubelet::state::common::registered::Registered;
use kubelet::state::common::terminated::Terminated;
use kubelet::state::common::{GenericProvider, GenericProviderState};
use kubelet::store::Store;
use kubelet::volume::VolumeRef;
use tokio::sync::RwLock;
use wasi_runtime::Runtime;

mod states;
use kubelet::node;
use states::pod::PodState;

const TARGET_WASM32_WASI: &str = "wasm32-wasi";
const LOG_DIR_NAME: &str = "wasi-logs";
const VOLUME_DIR: &str = "volumes";

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec.
#[derive(Clone)]
pub struct WasiProvider {
    shared: ProviderState,
}

type PodHandleMap = Arc<RwLock<HashMap<PodKey, Arc<Handle<Runtime, wasi_runtime::HandleFactory>>>>>;

/// Provider-level state shared between all pods
#[derive(Clone)]
pub struct ProviderState {
    handles: PodHandleMap,
    store: Arc<dyn Store + Sync + Send>,
    log_path: PathBuf,
    client: kube::Client,
    volume_path: PathBuf,
    plugin_registry: Arc<PluginRegistry>,
    device_plugin_manager: Arc<DeviceManager>,
}

#[async_trait]
impl GenericProviderState for ProviderState {
    fn client(&self) -> kube::client::Client {
        self.client.clone()
    }
    fn store(&self) -> std::sync::Arc<(dyn Store + Send + Sync + 'static)> {
        self.store.clone()
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

impl VolumeSupport for ProviderState {
    fn volume_path(&self) -> Option<&Path> {
        Some(self.volume_path.as_ref())
    }
}

impl PluginSupport for ProviderState {
    fn plugin_registry(&self) -> Option<Arc<PluginRegistry>> {
        Some(self.plugin_registry.clone())
    }
}

impl DevicePluginSupport for ProviderState {
    fn device_plugin_manager(&self) -> Option<Arc<DeviceManager>> {
        Some(self.device_plugin_manager.clone())
    }
}

impl WasiProvider {
    /// Create a new wasi provider from a module store and a kubelet config
    pub async fn new(
        store: Arc<dyn Store + Sync + Send>,
        config: &kubelet::config::Config,
        kubeconfig: kube::Config,
        plugin_registry: Arc<PluginRegistry>,
        device_plugin_manager: Arc<DeviceManager>,
    ) -> anyhow::Result<Self> {
        let log_path = config.data_dir.join(LOG_DIR_NAME);
        let volume_path = config.data_dir.join(VOLUME_DIR);
        tokio::fs::create_dir_all(&log_path).await?;
        tokio::fs::create_dir_all(&volume_path).await?;
        let client = kube::Client::try_from(kubeconfig)?;
        Ok(Self {
            shared: ProviderState {
                handles: Default::default(),
                store,
                log_path,
                volume_path,
                client,
                plugin_registry,
                device_plugin_manager,
            },
        })
    }
}

struct ModuleRunContext {
    modules: HashMap<String, Vec<u8>>,
    volumes: HashMap<String, VolumeRef>,
    env_vars: HashMap<String, HashMap<String, String>>,
}

#[async_trait::async_trait]
impl Provider for WasiProvider {
    type ProviderState = ProviderState;
    type InitialState = Registered<Self>;
    type TerminatedState = Terminated<Self>;
    type PodState = PodState;

    const ARCH: &'static str = TARGET_WASM32_WASI;

    fn provider_state(&self) -> SharedState<ProviderState> {
        Arc::new(RwLock::new(self.shared.clone()))
    }

    async fn node(&self, builder: &mut Builder) -> anyhow::Result<()> {
        builder.set_architecture("wasm-wasi");
        builder.add_taint("NoSchedule", "kubernetes.io/arch", Self::ARCH);
        builder.add_taint("NoExecute", "kubernetes.io/arch", Self::ARCH);
        Ok(())
    }

    async fn initialize_pod_state(&self, pod: &Pod) -> anyhow::Result<Self::PodState> {
        Ok(PodState::new(pod))
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

    // Evict all pods upon shutdown
    async fn shutdown(&self, node_name: &str) -> anyhow::Result<()> {
        node::drain(&self.shared.client, node_name).await?;
        Ok(())
    }
}

impl GenericProvider for WasiProvider {
    type ProviderState = ProviderState;
    type PodState = PodState;
    type RunState = crate::states::pod::initializing::Initializing;

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
