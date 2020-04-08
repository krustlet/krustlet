//! An OCI Distribution client for fetching oci images from an OCI compliant remote store
#![deny(missing_docs)]

pub mod client;
pub mod errors;
pub mod manifest;
mod reference;

#[doc(inline)]
pub use client::Client;
#[doc(inline)]
pub use reference::Reference;
