//! A crate for building custom Kubernetes [kubelets](https://kubernetes.io/docs/reference/command-line-tools-reference/kubelet/).
//!
//! The crate provides the [`Provider`] trait for declaring a Kubelet backend
//! as well as a the [`Kubelet`] type which takes a [`Provider`] and runs
//! a Kubelet server.
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Provider, Pod, Kubelet, config::Config};
//! use kube::client::APIClient;
//!
//! struct MyProvider;
//!
//! #[async_trait::async_trait]
//! impl Provider for MyProvider {
//!     fn arch(&self) -> String {
//!         "my-arch".to_string()
//!     }
//!
//!    async fn add(&self, pod: Pod, client: APIClient) -> anyhow::Result<()> {
//!        todo!("Implement Provider::add")
//!     }
//!
//!     // Implement the rest of the methods
//!     # fn can_schedule(&self, pod: &Pod) -> bool { todo!() }
//!     # async fn modify(&self, pod: Pod, client: APIClient) -> anyhow::Result<()> { todo!() }
//!     # async fn delete(&self, pod: Pod, client: APIClient) -> anyhow::Result<()> { todo!() }
//!     # async fn logs(&self, namespace: String, pod: String, container: String) -> anyhow::Result<Vec<u8>> { todo!() }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let provider = MyProvider;
//!     let kubeconfig = kube::config::load_kube_config().await.unwrap();
//!     let kubelet_config = Config::default();
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config);
//!
//!     kubelet.start().await.unwrap();
//! }
//! ```

#![warn(missing_docs)]
#![cfg_attr(feature = "docs", feature(doc_cfg))]

pub mod config;
pub mod image_client;
mod kubelet;
pub mod module_store;
mod node;
mod pod;
pub mod provider;
mod server;
pub mod status;

pub use self::kubelet::Kubelet;

#[doc(inline)]
pub use pod::Pod;
#[doc(inline)]
pub use provider::Provider;
