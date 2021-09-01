// Copied from the grpc_sock module in the Kubelet crate. The windows stuff is pretty hacky so it
// shouldn't be exported from there. Before we make this cross platform in the future, we'll need to
// make sure the server part works well on Windows
pub mod client;
pub mod server;
