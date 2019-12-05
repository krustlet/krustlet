/// Server is an HTTP(S) server for answering Kubelet callbacks.
/// 
/// Logs and exec calls are the main things that a server should handle.
use futures::future;
use hyper::rt::Future;
use hyper::service::service_fn;
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use log::{error,info};

/// Start the Krustlet HTTP(S) server
pub fn start_webserver() -> Result<(), failure::Error> {
    let addr = std::env::var("POD_IP")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
        .parse()?;
    let server = Server::bind(&addr)
        .serve(|| service_fn(pod_handler))
        .map_err(|e| error!("HTTP server error: {}", e));

    info!("starting webserver at: {:?}", &addr);
    hyper::rt::run(server);
    Ok(())
}

/// Convenience type for hyper
type BoxFut = Box<dyn futures::future::Future<Item = Response<Body>, Error = hyper::Error> + Send>;

/// Handler for all of the Pod-related HTTP Kubelet requests
///
/// Currently this implements:
/// - containerLogs
/// - exec
fn pod_handler(req: Request<Body>) -> BoxFut {
    let path: Vec<&str> = req.uri().path().split('/').collect();
    let path_len = path.len();
    if path_len < 2 {
        return Box::new(future::ok(get_ping()));
    }
    let res = match (req.method(), path[1], path_len) {
        (&Method::GET, "containerLogs", 5) => get_container_logs(req),
        (&Method::POST, "exec", 5) => post_exec(req),
        _ => {
            let mut response = Response::new(Body::from("Not Found"));
            *response.status_mut() = StatusCode::NOT_FOUND;
            response
        }
    };
    Box::new(future::ok(res))
}

/// Return a simple status message
fn get_ping() -> Response<Body> {
    Response::new(Body::from("this is the Krustlet HTTP server"))
}

/// Get the logs from the running WASM module
///
/// Implements the kubelet path /containerLogs/{namespace}/{pod}/{container}
fn get_container_logs(_req: Request<Body>) -> Response<Body> {
    Response::new(Body::from("{}"))
}
/// Run a pod exec command and get the output
///
/// Implements the kubelet path /exec/{namespace}/{pod}/{container}
fn post_exec(_req: Request<Body>) -> Response<Body> {
    let mut res = Response::new(Body::from("Not Implemented"));
    *res.status_mut() = StatusCode::NOT_IMPLEMENTED;
    res
}