//! An OCI Distribution client for fetching oci images from an OCI compliant remote store
#![cfg_attr(not(test), deny(missing_docs))]

#[cfg(test)]
use rstest_reuse;

pub mod client;
pub mod errors;
pub mod manifest;
mod reference;

#[doc(inline)]
pub use client::Client;
#[doc(inline)]
pub use reference::Reference;
