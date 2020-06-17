//! `oci` implements different storage methods for fetching modules from an OCI registry.
mod client;
mod file;

pub use client::Client;
pub use file::FileStore;
