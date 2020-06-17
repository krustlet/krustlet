use crate::config::ServerConfig;
use crate::log::{Options, Sender};
use crate::provider::{NotImplementedError, Provider};
use http::status::StatusCode;
use http::Response;
use hyper::Body;
/// Server is an HTTP(S) server for answering Kubelet callbacks.
///
/// Logs and exec calls are the main things that a server should handle.
use log::{debug, error};
use std::convert::Infallible;
use std::sync::Arc;
use warp::Filter;

const PING: &str = "this is the Krustlet HTTP server";

/// Start the Krustlet HTTP(S) server
///
/// This is a primitive implementation of an HTTP provider for the internal API.
pub(crate) async fn start<T: 'static + Provider + Send + Sync>(
    provider: Arc<T>,
    config: &ServerConfig,
) -> anyhow::Result<()> {
    let health = warp::get().and(warp::path("healthz")).map(|| PING);
    let ping = warp::get().and(warp::path::end()).map(|| PING);

    let logs_provider = provider.clone();
    let logs = warp::get()
        .and(warp::path!("containerLogs" / String / String / String))
        .and(warp::query::<Options>())
        .and_then(move |namespace, pod, container, opts| {
            let provider = logs_provider.clone();
            get_container_logs(provider, namespace, pod, container, opts)
        });

    let exec_provider = provider.clone();
    let exec = warp::post()
        .and(warp::path!("exec" / String / String / String))
        .and_then(move |namespace, pod, container| {
            let provider = exec_provider.clone();
            post_exec(provider, namespace, pod, container)
        });

    let routes = ping.or(health).or(logs).or(exec);

    warp::serve(routes)
        .tls()
        .cert_path(&config.cert_file)
        .key_path(&config.private_key_file)
        .run((config.addr, config.port))
        .await;
    Ok(())
}

/// Get the logs from the running container.
///
/// Implements the kubelet path /containerLogs/{namespace}/{pod}/{container}
async fn get_container_logs<T: 'static + Provider + Send + Sync>(
    provider: Arc<T>,
    namespace: String,
    pod: String,
    container: String,
    opts: Options,
) -> Result<Response<Body>, Infallible> {
    debug!(
        "Got container log request for container {} in pod {} in namespace {}. Options: {:?}.",
        container, pod, namespace, opts
    );
    let (sender, log_body) = Body::channel();
    let log_sender = Sender::new(sender, opts);

    match provider.logs(namespace, pod, container, log_sender).await {
        Ok(()) => Ok(Response::new(log_body)),
        Err(e) => {
            error!("Error fetching logs: {}", e);
            if e.is::<NotImplementedError>() {
                return_with_code(
                    StatusCode::NOT_IMPLEMENTED,
                    "Logs not implemented in provider.".to_owned(),
                )
            } else {
                return_with_code(
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
async fn post_exec<T: 'static + Provider + Send + Sync>(
    _provider: Arc<T>,
    _namespace: String,
    _pod: String,
    _container: String,
) -> Result<Response<Body>, Infallible> {
    return_with_code(
        StatusCode::NOT_IMPLEMENTED,
        "Exec not implemented.".to_string(),
    )
}

fn return_with_code(code: StatusCode, body: String) -> Result<Response<Body>, Infallible> {
    let mut response = Response::new(body.into());
    *response.status_mut() = code;
    Ok(response)
}
