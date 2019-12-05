use env_logger;
use futures::future;
use hyper::rt::Future;
use hyper::service::service_fn;
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use k8s_openapi::api::core::v1::{PodSpec, PodStatus};
use kube::{
    api::{Api, Informer, Object, PatchParams, WatchEvent},
    client::APIClient,
    config,
};
use log::{error, info};
use wasmtime::*;
use wasmtime_wasi::*;

use krustlet::node::*;

type KubePod = Object<PodSpec, PodStatus>;




fn main() -> Result<(), failure::Error> {
    let kubeconfig = config::load_kube_config()
        .or_else(|_| config::incluster_config())
        .expect("kubeconfig failed to load");
    let client = APIClient::new(kubeconfig);
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());

    env_logger::init();

    // Register as a node.
    create_node(client.clone());

    // Start updating the node periodically
    let update_client = client.clone();
    let node_updater = std::thread::spawn(move || {
        let sleep_interval = std::time::Duration::from_secs(10);
        loop {
            udpate_node(update_client.clone());
            std::thread::sleep(sleep_interval);
        }
    });

    let pod_informer = std::thread::spawn(move || {
        let resource = Api::v1Pod(client.clone()).within(namespace.as_str());

        // Create our informer and start listening.
        let informer = Informer::new(resource)
            .init()
            .expect("informer init failed");
        loop {
            informer.poll().expect("informer poll failed");
            while let Some(event) = informer.pop() {
                handle(client.clone(), event, namespace.clone());
            }
        }
    });

    start_webserver()?;

    node_updater.join().expect("node update thread crashed");

    pod_informer.join().expect("informer thread crashed");

    Ok(())
}
/// Handle each event on a pod
fn handle(client: APIClient, event: WatchEvent<KubePod>, namespace: String) {
    match event {
        WatchEvent::Added(p) => {
            info!("Pod added");
            // Start with a hard-coded WASM file
            let data = std::fs::read("./examples/greet.wasm")
                .expect("greet.wasm should be in examples directory");
            pod_status(client.clone(), p.clone(), "Running", namespace.as_str());
            match wasm_run(&data) {
                Ok(_) => {
                    info!("Pod run to completion");
                    pod_status(client.clone(), p, "Succeeded", namespace.as_str());
                }
                Err(e) => {
                    error!("Failed to run pod: {}", e);
                    pod_status(client.clone(), p, "Failed", namespace.as_str());
                }
            }
        }
        WatchEvent::Modified(p) => {
            info!("Pod modified");
            info!(
                "Modified pod spec: {}",
                serde_json::to_string_pretty(&p.status.unwrap()).unwrap()
            );
        }
        WatchEvent::Deleted(p) => {
            pod_status(client.clone(), p, "Succeeded", namespace.as_str());
            println!("Pod deleted")
        }
        WatchEvent::Error(e) => println!("Error: {}", e),
    }
}

fn pod_status(client: APIClient, pod: KubePod, phase: &str, ns: &str) {
    let status = serde_json::json!(
        {
            "metadata": {
                "resourceVersion": "",
            },
            "status": {
                "phase": phase
            }
        }
    );

    let meta = pod.metadata.clone();
    let pp = PatchParams::default();
    let data = serde_json::to_vec(&status).expect("Should always serialize");
    match Api::v1Pod(client)
        .within(ns)
        .patch_status(meta.name.as_str(), &pp, data)
    {
        Ok(o) => {
            info!("Pod status for {} set to {}", meta.name.as_str(), phase);
            info!(
                "Pod status returned: {}",
                serde_json::to_string_pretty(&o.status).unwrap()
            )
        }
        Err(e) => error!("Pod status update failed for {}: {}", meta.name.as_str(), e),
    }
}



/// Start the Krustlet HTTP(S) server
fn start_webserver() -> Result<(), failure::Error> {
    let addr = std::env::var("POD_IP")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
        .parse()?;
    let server = Server::bind(&addr)
        .serve(|| service_fn(pod_handler))
        .map_err(|e| error!("HTTP server error: {}", e));

    println!("starting webserver at: {:?}", &addr);
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

fn wasm_run(data: &[u8]) -> Result<(), failure::Error> {
    let engine = HostRef::new(Engine::default());
    let store = HostRef::new(Store::new(&engine));
    let module = HostRef::new(Module::new(&store, data).expect("wasm module"));
    let preopen_dirs = vec![];
    let argv = vec![];
    let environ = vec![];
    // Build a list of WASI modules
    let wasi_inst = HostRef::new(create_wasi_instance(
        &store,
        &preopen_dirs,
        &argv,
        &environ,
    )?);
    // Iterate through the module includes and resolve imports
    let imports = module
        .borrow()
        .imports()
        .iter()
        .map(|i| {
            let module_name = i.module().as_str();
            let field_name = i.name().as_str();
            if let Some(export) = wasi_inst.borrow().find_export_by_name(field_name) {
                Ok(export.clone())
            } else {
                failure::bail!(
                    "Import {} was not found in module {}",
                    field_name,
                    module_name
                )
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Then create the instance
    let _instance = Instance::new(&store, &module, &imports).expect("wasm instance");

    info!("Instance was executed");
    Ok(())
}
