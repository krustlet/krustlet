/// Server is an HTTP(S) server for answering Kubelet callbacks.
///
/// Logs and exec calls are the main things that a server should handle.
use anyhow::Context;
use async_stream::stream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{
    server::{conn::Http, Builder},
    Body, Error, Method, Request, Response, StatusCode,
};
use log::{debug, error, info};
use native_tls::{Identity, TlsAcceptor};
use tokio::net::TcpListener;
use tokio::stream::StreamExt;
use tokio::sync::Mutex;

use std::sync::Arc;

use crate::config::ServerConfig;
use crate::kubelet::Provider;

/// Start the Krustlet HTTP(S) server
///
/// This is a primitive implementation of an HTTP provider for the internal API.
/// TODO: Support TLS/SSL.
pub async fn start_webserver<T: 'static + Provider + Send + Sync>(
    provider: Arc<Mutex<T>>,
    config: &ServerConfig,
) -> anyhow::Result<()> {
    let identity = tokio::fs::read(&config.pfx_path)
        .await
        .with_context(|| format!("Could not read file {:?}", config.pfx_path))?;
    let identity = Identity::from_pkcs12(&identity, &config.pfx_password)?;

    let acceptor = tokio_tls::TlsAcceptor::from(TlsAcceptor::new(identity)?);
    let acceptor = Arc::new(acceptor);
    let service = make_service_fn(move |_| {
        let provider = provider.clone();
        async {
            Ok::<_, Error>(service_fn(move |req: Request<Body>| {
                let provider = provider.clone();

                async move {
                    let path: Vec<&str> = req.uri().path().split('/').collect();

                    let response = match (req.method(), path.as_slice()) {
                        (_, path) if path.len() <= 2 => get_ping(),
                        (&Method::GET, [_, "containerLogs", namespace, pod, container]) => {
                            get_container_logs(
                                &*provider.lock().await,
                                &req,
                                namespace.to_string(),
                                pod.to_string(),
                                container.to_string(),
                            )
                            .await
                        }
                        (&Method::POST, [_, "exec", _, _, _]) => {
                            post_exec(&*provider.lock().await, &req)
                        }
                        _ => Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Body::from("Not Found"))
                            .unwrap(),
                    };
                    Ok::<_, Error>(response)
                }
            }))
        }
    });

    let address = std::net::SocketAddr::new(config.addr, config.port);
    let mut listener = TcpListener::bind(&address).await.unwrap();
    let mut incoming = listener.incoming();
    let accept = hyper::server::accept::from_stream(stream! {
        loop {
            match incoming.next().await {
                Some(Ok(stream)) => match acceptor.clone().accept(stream).await {
                    result @ Ok(_) => yield result,
                    Err(e) => error!("error accepting ssl connection: {}", e),
                },
                _ => break,
            }
        }
    });
    let server = Builder::new(accept, Http::new()).serve(service);

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
    namespace: String,
    pod: String,
    container: String,
) -> Response<Body> {
    debug!(
        "Got container log request for container {} in pod {} in namespace {}",
        container, pod, namespace
    );
    if namespace.is_empty() || pod.is_empty() || container.is_empty() {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(format!(
                "Resource {} not found",
                req.uri().path()
            )))
            .unwrap();
    }
    match provider.logs(namespace, pod, container).await {
        Ok(data) => Response::new(Body::from(data)),
        // TODO: This should detect not implemented vs. regular error
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
