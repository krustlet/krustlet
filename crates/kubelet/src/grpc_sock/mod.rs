//! A client/server implementation using UNIX sockets for gRPC, meant for use with tonic. Socket
//! support is not built in to tonic and support for UNIX sockets on Windows requires its own crate
//! (as it isn't in standard due to backwards compatibility guarantees). This is our own package for
//! now, but if it is useful we could publish it as its own crate

// Right now we only use the server for testing purposes. If we choose to publish this as its own
// crate, we should remove this attribute
#[cfg_attr(target_family = "unix", path = "unix/mod.rs")]
#[cfg_attr(target_family = "windows", path = "windows/mod.rs")]
// #[cfg(test)]
pub mod server;

// TODO: Figure out what to export

pub mod client;
