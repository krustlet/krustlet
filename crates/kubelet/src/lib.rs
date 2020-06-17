//! A crate for building custom Kubernetes [kubelets](https://kubernetes.io/docs/reference/command-line-tools-reference/kubelet/).
//!
//! The crate provides the [`Provider`] trait for declaring a Kubelet backend
//! as well as a the [`Kubelet`] type which takes a [`Provider`] and runs
//! a Kubelet server.
//!
//! # Example
//! ```rust,no_run
//! use kubelet::Kubelet;
//! use kubelet::config::Config;
//! use kubelet::pod::Pod;
//! use kubelet::provider::Provider;
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
//!     # async fn logs(&self, namespace: String, pod: String, container: String, sender: kubelet::log::Sender) -> anyhow::Result<()> { todo!() }
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
mod kubelet;

pub(crate) mod kubeconfig;
pub(crate) mod webserver;

pub mod config;
pub mod container;
pub mod handle;
pub mod log;
pub mod node;
pub mod pod;
pub mod provider;
pub mod store;
pub mod volume;

pub use self::kubelet::Kubelet;
pub use bootstrapping::bootstrap;
