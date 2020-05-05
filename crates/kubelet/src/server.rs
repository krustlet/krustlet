/// Server is an HTTP(S) server for answering Kubelet callbacks.
///
/// Logs and exec calls are the main things that a server should handle.
use anyhow::Context;
use hyper::service::service_fn;
use hyper::{server::conn::Http, Body, Method, Request, Response, StatusCode};
use log::{debug, error, info, warn};
use native_tls::{Identity, TlsAcceptor};
use tokio::net::{TcpListener, TcpStream};
use tokio::stream::StreamExt;

use std::sync::Arc;

use crate::config::ServerConfig;
use crate::logs::LogSender;
use crate::provider::{NotImplementedError, Provider};

/// Start the Krustlet HTTP(S) server
///
/// This is a primitive implementation of an HTTP provider for the internal API.
/// TODO: Support TLS/SSL.
pub async fn start_webserver<T: 'static + Provider + Send + Sync>(
    provider: Arc<T>,
    config: &ServerConfig,
) -> anyhow::Result<()> {
    let identity = tokio::fs::read(&config.pfx_path)
        .await
        .with_context(|| format!("Could not read file {:?}", config.pfx_path))?;
    let identity = Identity::from_pkcs12(&identity, &config.pfx_password)?;

    let acceptor = tokio_tls::TlsAcceptor::from(TlsAcceptor::new(identity)?);

    let acceptor = Arc::new(acceptor);

    let address = std::net::SocketAddr::new(config.addr, config.port);
    let mut listener = TcpListener::bind(&address).await.unwrap();

    info!("Starting webserver at: {}", address);

    let mut incoming = listener.incoming();

    while let Some(conn) = incoming.try_next().await? {
        let acceptor = acceptor.clone();
        let provider = provider.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(conn, acceptor, provider).await {
                error!("Error handling server connection: {}", e);
            }
        });
    }

    Ok(())
}

async fn handle_connection<T>(
    conn: TcpStream,
    acceptor: Arc<tokio_tls::TlsAcceptor>,
    provider: Arc<T>,
) -> anyhow::Result<()>
where
    T: Provider + Send + Sync + 'static,
{
    let io = acceptor.accept(conn).await?;
    Http::new()
        .serve_connection(
            io,
            service_fn(move |req| {
                let provider = provider.clone();
                async move { handle_request(req, provider).await }
            }),
        )
        .await?;

    Ok(())
}

async fn handle_request<T>(req: Request<Body>, provider: Arc<T>) -> anyhow::Result<Response<Body>>
where
    T: Provider + Send + Sync + 'static,
{
    let path: Vec<&str> = req.uri().path().split('/').collect();

    let response = match (req.method(), path.as_slice()) {
        (_, path) if path.len() <= 2 => get_ping(),
        (&Method::GET, [_, "containerLogs", namespace, pod, container]) => {
            let params: std::collections::HashMap<String, String> = req
                .uri()
                .query()
                .map(|v| {
                    url::form_urlencoded::parse(v.as_bytes())
                        .into_owned()
                        .collect()
                })
                .unwrap_or_else(std::collections::HashMap::new);
            let mut tail = None;
            let mut follow = false;
            for (key, value) in &params {
                match key.as_ref() {
                    "tailLines" => match value.parse::<usize>() {
                        Ok(n) => tail = Some(n),
                        Err(e) => {
                            warn!(
                                "Unable to parse tailLines query parameter ({}): {:?}",
                                value, e
                            );
                        }
                    },
                    "follow" if value == "true" => follow = true,
                    s => warn!("Unknown query parameter: {}={}", s, value),
                }
            }
            get_container_logs(
                &*provider,
                &req,
                (*namespace).to_string(),
                (*pod).to_string(),
                (*container).to_string(),
                tail,
                follow,
            )
            .await
        }
        (&Method::POST, [_, "exec", _, _, _]) => post_exec(&*provider, &req),
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    };

    Ok(response)
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
    tail: Option<usize>,
    follow: bool,
) -> Response<Body> {
    debug!(
        "Got container log request for container {} in pod {} in namespace {}. tail: {:?}, follow: {}",
        container, pod, namespace, tail, follow
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
    let (sender, log_body) = hyper::Body::channel();
    let log_sender = LogSender::new(sender);

    match provider
        .logs(namespace, pod, container, log_sender, tail, follow)
        .await
    {
        Ok(()) => Response::new(log_body),
        Err(e) => {
            error!("Error fetching logs: {}", e);
            let mut res = Response::new(Body::from(format!("Server error: {}", e)));
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            if e.is::<NotImplementedError>() {
                res = Response::new(Body::from("Not Implemented"));
                *res.status_mut() = StatusCode::NOT_IMPLEMENTED;
            }
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
