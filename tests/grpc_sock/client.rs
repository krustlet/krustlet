// This is heavily adapted from https://github.com/hyperium/tonic/blob/f1275b611e38ec5fe992b2f10552bf95e8448b17/examples/src/uds/client.rs
use std::path::Path;
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

/// Returns a new UNIX socket channel suitable for use with tonic generated gRPC clients. Instead of
/// using `YourClient::connect`, you'll need to pass the returned channel to `YourClient::new`
pub async fn socket_channel<P: AsRef<Path>>(path: P) -> Result<Channel, tonic::transport::Error> {
    // Get an owned copy of the path so we can use it in the FnMut closure
    let p = path.as_ref().to_owned();

    // This is a dummy http endpoint needed for the Endpoint constructors, it is ignored by the
    // connector
    #[cfg(target_family = "unix")]
    let res = Endpoint::from_static("http://[::]:50051")
        .connect_with_connector(service_fn(move |_: Uri| {
            // Connect to a Uds socket
            UnixStream::connect(p.clone())
        }))
        .await;

    res
}
