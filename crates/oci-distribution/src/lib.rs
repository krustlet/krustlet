//! An OCI Distribution client for fetching oci images from an OCI compliant remote store
#![deny(missing_docs)]

pub mod client;
pub mod errors;
pub mod manifest;
mod reference;
mod regexp;
pub mod secrets;

#[doc(inline)]
pub use client::Client;
#[doc(inline)]
pub use reference::Reference;

#[macro_use]
extern crate lazy_static;
