//! A crate for building custom Kubernetes [kubelets](https://kubernetes.io/docs/reference/command-line-tools-reference/kubelet/).
//!
//! The crate provides the [`Provider`] trait for declaring a Kubelet backend
//! as well as a the [`Kubelet`] type which takes a [`Provider`] and runs
//! a Kubelet server.
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Provider, Pod, Kubelet, config::Config};
//!
//! // Create some type that will act as your provider
//! struct MyProvider;
//!
//! // Implement the `Provider` trait for that type
//! #[async_trait::async_trait]
//! impl Provider for MyProvider {
//!    const ARCH: &'static str = "my-arch";
//!
//!    async fn add(&self, pod: Pod) -> anyhow::Result<()> {
//!        todo!("Implement Provider::add")
//!     }
//!
//!     // Implement the rest of the methods
//!     # async fn modify(&self, pod: Pod) -> anyhow::Result<()> { todo!() }
//!     # async fn delete(&self, pod: Pod) -> anyhow::Result<()> { todo!() }
//!     # async fn logs(&self, namespace: String, pod: String, container: String) -> anyhow::Result<Vec<u8>> { todo!() }
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
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config);
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! };
//! ```

#![deny(missing_docs)]
#![cfg_attr(feature = "docs", feature(doc_cfg))]

mod kubelet;
mod node;
mod pod;
mod queue;
mod server;

pub mod config;
pub mod handle;
pub mod image_client;
pub mod module_store;
pub mod provider;
pub mod status;
pub mod volumes;

pub use self::kubelet::Kubelet;
pub use handle::{PodHandle, RuntimeHandle};
pub use pod::Pod;
#[doc(inline)]
pub use provider::Provider;
