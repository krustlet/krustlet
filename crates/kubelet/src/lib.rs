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
//! // Create some type that will act as your provider
//! struct MyProvider;
//!
//! // Implement the `Provider` trait for that type
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
//!     // Instantiate your provider type
//!     let provider = MyProvider;
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::config::load_kube_config().await.unwrap();
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config);
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! }
//! ```

#![warn(missing_docs)]
#![cfg_attr(feature = "docs", feature(doc_cfg))]

mod kubelet;
mod node;
mod pod;
mod server;

pub mod config;
pub mod image_client;
pub mod module_store;
pub mod provider;
pub mod handle;
pub mod status;

pub use self::kubelet::Kubelet;
pub use pod::Pod;
pub use handle::{RuntimeHandle, PodHandle};
#[doc(inline)]
pub use provider::Provider;
 "thiserror",
 "wast 9.0.0",
]

[[package]]
name = "ws2_32-sys"
version = "0.2.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "d59cefebd0c892fa2dd6de581e937301d8552cb44489cdff035c6187cb63fa5e"
dependencies = [
 "winapi 0.2.8",
 "winapi-build",
]

[[package]]
name = "www-authenticate"
version = "0.3.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "8c62efb8259cda4e4c732287397701237b78daa4c43edcf3e613c8503a6c07dd"
dependencies = [
 "hyperx",
 "unicase 1.4.2",
 "url 1.7.2",
]

[[package]]
name = "yaml-rust"
version = "0.4.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "65923dd1784f44da1d2c3dbbc5e822045628c590ba72123e1c73d3c230c4434d"
dependencies = [
 "linked-hash-map",
]

[[package]]
name = "yanix"
version = "0.12.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "d9a936d5291b6269cf230b50fb995c5d5ff6d775f8efaa75234b26b88e5e3e78"
dependencies = [
 "bitflags",
 "cfg-if",
 "libc",
 "log",
 "thiserror",
]

[[package]]
name = "zeroize"
version = "1.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "3cbac2ed2ba24cc90f5e06485ac8c7c1e5449fe8911aef4d8877218af021a5b8"

[[package]]
name = "zstd"
version = "0.5.1+zstd.1.4.4"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "5c5d978b793ae64375b80baf652919b148f6a496ac8802922d9999f5a553194f"
dependencies = [
 "zstd-safe",
]

[[package]]
name = "zstd-safe"
