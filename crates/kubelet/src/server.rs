/// Server is an HTTP(S) server for answering Kubelet callbacks.
///
/// Logs and exec calls are the main things that a server should handle.
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Method, Request, Response, Server, StatusCode};
use log::{debug, error, info};
use tokio::sync::Mutex;

use std::net::SocketAddr;
use std::sync::Arc;

use crate::kubelet::Provider;

/// Start the Krustlet HTTP(S) server
///
/// This is a primitive implementation of an HTTP provider for the internal API.
/// TODO: Support TLS/SSL.
pub async fn start_webserver<T: 'static + Provider + Send + Sync>(
    provider: Arc<Mutex<T>>,
    address: &SocketAddr,
) -> Result<(), failure::Error> {
    let service = make_service_fn(move |_conn: &AddrStream| {
        let provider = provider.clone();
        async {
            Ok::<_, Error>(service_fn(move |req: Request<Body>| {
                let provider = provider.clone();

                async move {
                    let path: Vec<&str> = req.uri().path().split('/').collect();
                    let path_len = path.len();
                    let response = if path_len < 2 {
                        get_ping()
                    } else {
                        match (req.method(), path[1], path_len) {
                            (&Method::GET, "containerLogs", 5) => {
                                get_container_logs(&*provider.lock().await, &req).await
                            }
                            (&Method::POST, "exec", 5) => post_exec(&*provider.lock().await, &req),
                            _ => {
                                let mut response = Response::new(Body::from("Not Found"));
                                *response.status_mut() = StatusCode::NOT_FOUND;
                                response
                            }
                        }
                    };
                    Ok::<_, Error>(response)
                }
            }))
        }
    });
    let server = Server::bind(address).serve(service);

    info!("starting webserver at: {:?}", address);

    server.await?;

    Ok(())
}

/// Return a simple status message
fn get_ping() -> Response<Body> {
    Response::new(Body::from("this is the Krustlet HTTP server"))
}

/// Get the logs from the running WASM module
///
/// Implements the kubelet path /containerLogs/{namespace}/{pod}/{container}
async fn get_container_logs<T: Provider + Sync>(
    provider: &T,
    req: &Request<Body>,
) -> Response<Body> {
    // Basic validation steps
    let path: Vec<&str> = req.uri().path().split('/').collect();
    // Because of the leading slash, index 0 is an empty string. Index 1 is the
    // container logs path
    if path.len() != 5 {
        let mut res = Response::new(Body::from(format!(
            "Resource {} not found",
            req.uri().path()
        )));
        *res.status_mut() = StatusCode::NOT_FOUND;
        return res;
    }

    let namespace = path[2];
    let pod = path[3];
    let container = path[4];
    if namespace.is_empty() || pod.is_empty() || container.is_empty() {
        let mut res = Response::new(Body::from(format!(
            "Resource {} not found",
            req.uri().path()
        )));
        *res.status_mut() = StatusCode::NOT_FOUND;
        return res;
    }

    // END validation

    debug!(
        "Got container log request for container {} in pod {} in namespace {}",
        container, pod, namespace
    );

    match provider
        .logs(namespace.into(), pod.into(), container.into())
        .await
    {
        Ok(data) => Response::new(Body::from(data)),
        // TODO: This should detect not implemented vs. regular error (pod not found, etc.)
        Err(e) => {
            error!("Error fetching logs: {}", e);
            let mut res = Response::new(Body::from("Not Implemented"));
            *res.status_mut() = StatusCode::NOT_IMPLEMENTED;
            res
        }
    }
}
/// Run a pod exec command and get the output
///
/// Implements the kubelet path /exec/{namespace}/{pod}/{container}
fn post_exec<T: Provider>(_provider: &T, _req: &Request<Body>) -> Response<Body> {
    let mut res = Response::new(Body::from("Not Implemented"));
    *res.status_mut() = StatusCode::NOT_IMPLEMENTED;
    res
}
