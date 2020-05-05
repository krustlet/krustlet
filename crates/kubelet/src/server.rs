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
    let mut path: std::collections::VecDeque<&str> = req.uri().path().split('/').collect();
    let method = req.method();
    let params: std::collections::HashMap<String, String> = req
        .uri()
        .query()
        .map(|v| {
            url::form_urlencoded::parse(v.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_else(std::collections::HashMap::new);

    path.pop_front();
    let resource = path.pop_front();
    match resource {
        Some("") | Some("healthz") if method == &Method::GET => get_ping(),
        Some("containerLogs") if method == &Method::GET => {
            let (namespace, pod, container) = match extract_container_path(path.into()) {
                Ok(resource) => resource,
                Err(e) => return e,
            };
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
                    "follow" => (),
                    s => warn!("Unknown query parameter: {}={}", s, value),
                }
            }
            get_container_logs(&*provider, &req, namespace, pod, container, tail, follow).await
        }
        Some("exec") if method == &Method::POST => {
            let (namespace, pod, container) = match extract_container_path(path.into()) {
                Ok(resource) => resource,
                Err(e) => return e,
            };
            post_exec(&*provider, &req, namespace, pod, container).await
        }
        Some("tailLines") | Some("containerLogs") | Some("") | Some("healthz") => return_code(
            StatusCode::METHOD_NOT_ALLOWED,
            format!(
                "Unsupported method {} for resource '{}'.",
                method,
                req.uri().path()
            ),
        ),
        Some(_) | None => return_code(
            StatusCode::NOT_FOUND,
            format!("Unknown resource '{}'.", req.uri().path()),
        ),
    }
}

/// Extract and validate namespace/pod/container resource path.
/// On error return response to return to client.
fn extract_container_path(
    path: Vec<&str>,
) -> Result<(String, String, String), anyhow::Result<Response<Body>>> {
    match path[..] {
        [] => Err(return_code(
            StatusCode::BAD_REQUEST,
            format!("Please specify a namespace."),
        )),
        [_] => Err(return_code(
            StatusCode::BAD_REQUEST,
            format!("Please specify a pod."),
        )),
        [_, _] => Err(return_code(
            StatusCode::BAD_REQUEST,
            format!("Please specify a container."),
        )),
        [s, _, _] if s == "" => Err(return_code(
            StatusCode::BAD_REQUEST,
            format!("Please specify a namespace."),
        )),
        [_, s, _] if s == "" => Err(return_code(
            StatusCode::BAD_REQUEST,
            format!("Please specify a pod."),
        )),
        [_, _, s] if s == "" => Err(return_code(
            StatusCode::BAD_REQUEST,
            format!("Please specify a container."),
        )),
        [namespace, pod, container] => Ok((
            namespace.to_string(),
            pod.to_string(),
            container.to_string(),
        )),
        [_, _, _, ..] => {
            let resource = path
                .iter()
                .rev()
                .map(|s| *s)
                .collect::<Vec<&str>>()
                .join("/");
            Err(return_code(
                StatusCode::NOT_FOUND,
                format!("Unknown resource '{}'.", resource),
            ))
        }
    }
}

/// Return a simple status message
fn get_ping() -> anyhow::Result<Response<Body>> {
    Ok(Response::new(Body::from(
        "this is the Krustlet HTTP server",
    )))
}

/// Return a HTTP code and message.
fn return_code(code: StatusCode, body: String) -> anyhow::Result<Response<Body>> {
    Ok(Response::builder().status(code).body(Body::from(body))?)
}

/// Get the logs from the running WASM module
///
/// Implements the kubelet path /containerLogs/{namespace}/{pod}/{container}
async fn get_container_logs<T: Provider + Sync>(
    provider: &T,
    _req: &Request<Body>,
    namespace: String,
    pod: String,
    container: String,
    tail: Option<usize>,
    follow: bool,
) -> anyhow::Result<Response<Body>> {
    debug!(
        "Got container log request for container {} in pod {} in namespace {}. tail: {:?}, follow: {}",
        container, pod, namespace, tail, follow
    );

    let (sender, log_body) = hyper::Body::channel();
    let log_sender = LogSender::new(sender, tail, follow);

    match provider.logs(namespace, pod, container, log_sender).await {
        Ok(()) => Ok(Response::new(log_body)),
        Err(e) => {
            error!("Error fetching logs: {}", e);
            if e.is::<NotImplementedError>() {
                return_code(
                    StatusCode::NOT_IMPLEMENTED,
                    format!("Logs not implemented in provider."),
                )
            } else {
                return_code(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Server error: {}", e),
                )
            }
        }
    }
}

/// Run a pod exec command and get the output
///
/// Implements the kubelet path /exec/{namespace}/{pod}/{container}
async fn post_exec<T: Provider>(
    _provider: &T,
    _req: &Request<Body>,
    _namespace: String,
    _pod: String,
    _container: String,
) -> anyhow::Result<Response<Body>> {
    return_code(
        StatusCode::NOT_IMPLEMENTED,
        format!("Exec not implemented."),
    )
}
