//! A crate for building custom Kubernetes [kubelets](https://kubernetes.io/docs/reference/command-line-tools-reference/kubelet/).
//!
//! The crate provides the [`Provider`](crate::provider::Provider) trait for declaring a Kubelet
//! backend as well as a the [`Kubelet`] type which takes a [`Provider`](crate::provider::Provider)
//! and runs a Kubelet server.
//!
//! # Example
//! ```rust,no_run
//! use kubelet::Kubelet;
//! use kubelet::config::Config;
//! use kubelet::resources::DeviceManager;
//! use kubelet::plugin_watcher::PluginRegistry;
//! use kubelet::pod::Pod;
//! use kubelet::provider::{DevicePluginSupport, Provider, PluginSupport};
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//! use kubelet::pod::state::prelude::*;
//! use kubelet::pod::state::Stub;
//!
//! // Create some type that will act as your provider
//! struct MyProvider;
//!
//! // Track shared provider-level state across pods.
//! struct ProviderState;
//! // Track pod state amongst pod state handlers.
//! struct PodState;
//!
//! #[async_trait::async_trait]
//! impl ObjectState for PodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//!     type SharedState = ProviderState;
//!     async fn async_drop(self, _provider_state: &mut ProviderState) {}
//! }
//!
//! // Implement the `Provider` trait for that type
//! #[async_trait::async_trait]
//! impl Provider for MyProvider {
//!     const ARCH: &'static str = "my-arch";
//!     type ProviderState = ProviderState;
//!     type InitialState = Stub;
//!     type TerminatedState = Stub;
//!     type PodState = PodState;
//!
//!     fn provider_state(&self) -> SharedState<ProviderState> {
//!         Arc::new(RwLock::new(ProviderState {}))
//!     }
//!
//!     async fn initialize_pod_state(&self, _pod: &Pod) -> anyhow::Result<Self::PodState> {
//!         Ok(PodState)
//!     }
//!
//!     async fn logs(&self, namespace: String, pod: String, container: String, sender: kubelet::log::Sender) -> anyhow::Result<()> { todo!() }
//! }
//!
//! impl PluginSupport for ProviderState {
//!     fn plugin_registry(&self) -> Option<Arc<PluginRegistry>> {
//!         None
//!     }
//! }
//!
//! impl DevicePluginSupport for ProviderState {
//!     fn device_plugin_manager(&self) -> Option<Arc<DeviceManager>> {
//!         None
//!     }
//! }
//!
//! async {
//!     // Instantiate your provider type
//!     let provider = MyProvider;
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::Config::infer().await.unwrap();
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config).await.unwrap();
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! };
//! ```

#![deny(missing_docs)]
#![cfg_attr(feature = "docs", feature(doc_cfg))]

mod bootstrapping;
mod config_interpreter;
mod kubelet;
mod operator;

pub(crate) mod kubeconfig;
pub(crate) mod webserver;
pub(crate) mod plugin_registration_api {
    pub(crate) mod v1 {
        pub const API_VERSION: &str = "1.0.0";

        tonic::include_proto!("pluginregistration");
    }
}
pub(crate) mod device_plugin_api {
    pub(crate) mod v1beta1 {
        pub const API_VERSION: &str = "v1beta1";
        tonic::include_proto!("v1beta1");
    }
}
pub(crate) mod fs_watch;
pub(crate) mod grpc_sock;
#[cfg(target_family = "windows")]
#[allow(dead_code, clippy::all)]
pub(crate) mod mio_uds_windows;

pub mod backoff;
pub mod config;
pub mod container;
pub mod handle;
pub mod log;
pub mod node;
pub mod plugin_watcher;
pub mod pod;
pub mod provider;
pub mod resources;
pub mod secret;
pub mod state;
pub mod store;
pub mod volume;

pub use self::kubelet::Kubelet;
pub use bootstrapping::bootstrap;

#[cfg(feature = "derive")]
#[allow(unused_imports)]
#[macro_use]
extern crate krator;
