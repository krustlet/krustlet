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
//! use kubelet::state::{State, Transition};
//! use std::sync::Arc;
//!
//! // Create some type that will act as your provider
//! struct MyProvider;
//!
//! // Implement a state machine of Pod states
//! #[derive(Default)]
//! struct Completed;
//! #[async_trait::async_trait]
//! impl <P: 'static + Sync + Send> State<P> for Completed {
//!     type Success  = Completed;
//!     type Error = Completed;
//!
//!     async fn next(
//!         self,
//!         provider: Arc<P>,
//!         pod: &Pod,
//!     ) -> anyhow::Result<Transition<Self::Success, Self::Error>> {
//!         Ok(Transition::Complete(Ok(())))
//!     }
//!     
//!     async fn json_status(
//!         &self,
//!         provider: Arc<P>,
//!         pod: &Pod,
//!     ) -> anyhow::Result<serde_json::Value> {
//!         Ok(serde_json::json!(null))
//!     }
//! }
//!
//! // Implement the `Provider` trait for that type
//! #[async_trait::async_trait]
//! impl Provider for MyProvider {
//!     const ARCH: &'static str = "my-arch";
//!     type InitialState = Completed;
//!
//!     async fn modify(&self, pod: Pod) {
//!        todo!("Implement Provider::add")
//!     }
//!
//!     // Implement the rest of the methods
//!     async fn delete(&self, pod: Pod) { todo!() }
//!     async fn logs(&self, namespace: String, pod: String, container: String, sender: kubelet::log::Sender) -> anyhow::Result<()> { todo!() }
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
pub mod state;

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
