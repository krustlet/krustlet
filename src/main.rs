#[macro_use]
extern crate serde_json;
extern crate base64;

use chrono::prelude::*;
use futures::future;
use hyper::rt::Future;
use hyper::service::service_fn;
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use k8s_openapi::api::core::v1::{NodeSpec, NodeStatus, PodSpec, PodStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::{
    api::{Api, Informer, Object, PatchParams, PostParams, WatchEvent},
    client::APIClient,
    config,
};
use log::{error, info};

type KubePod = Object<PodSpec, PodStatus>;
type KubeNode = Object<NodeSpec, NodeStatus>;

const NODE_NAME: &str = "krustlet";

fn main() -> Result<(), failure::Error> {
    let kubeconfig = config::load_kube_config()
        .or_else(|_| config::incluster_config())
        .expect("kubeconfig failed to load");
    let client = APIClient::new(kubeconfig);
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());

    // Register as a node.
    create_node(client.clone());

    // Start updating the node periodically
    let update_client = client.clone();
    let node_updater = std::thread::spawn(move || {
        let sleep_interval = std::time::Duration::from_secs(30);
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
fn handle(_client: APIClient, event: WatchEvent<KubePod>, _namespace: String) {
    match event {
        WatchEvent::Added(_o) => println!("Pod added"),
        WatchEvent::Modified(_o) => println!("Pod modified"),
        WatchEvent::Deleted(_o) => println!("Pod deleted"),
        WatchEvent::Error(e) => println!("Error: {}", e),
    }
}

/// Create a node
///
/// This creates a Kubernetes Node that describes our Kubelet, failing with a log message
/// if one already exists. If one does exist, we simply re-use it. You may call that
/// hacky, but I call it... hacky.
fn create_node(client: APIClient) {
    let node_client = Api::v1Node(client);
    let pp = PostParams::default();
    let node = json!({
        "apiVersion": "v1",
        "kind": "Node",
        "metadata": {
            "name": NODE_NAME,
            "labels": {
                "beta.kubernetes.io/arch": "wasm-wasi",
                "beta.kubernetes.io/os": "linux",
                "kubernetes.io/arch": "wasm-wasi",
                "kubernetes.io/os": "linux",
                "kubernetes.io/hostname": "krustlet",
                "kubernetes.io/role":     "agent",
                "type": "krustlet"
            },
            "annotations": {
                "node.alpha.kubernetes.io/ttl": "0",
                "volumes.kubernetes.io/controller-managed-attach-detach": "true"
            }
        },
        "spec": {
            "podCIDR": "10.244.0.0/24"
        },
        "status": {
            "nodeInfo": {
                "operatingSystem": "linux",
                "architecture": "wasm-wasi",
                "kubeletVersion": "v1.15.0",
            },
            "capacity": {
                "cpu": "4",
                "ephemeral-storage": "61255492Ki",
                "hugepages-1Gi": "0",
                "hugepages-2Mi": "0",
                "memory": "4032800Ki",
                "pods": "30"
            }
        }
    });
    match node_client.create(
        &pp,
        serde_json::to_vec(&node).expect("node serializes correctly"),
    ) {
        Ok(_) => println!("created node just fine"),
        Err(e) => println!("Error creating node: {}", e),
    };
}

/// Update the timestamps on the Node object.
///
/// This is how we report liveness to the upstream.
///
/// We trap errors because... well... quite frankly there is nothing useful
/// to do if the Kubernetes API is unavailable, and we can merrily continue
/// doing our processing of the pod queue.
fn udpate_node(client: APIClient) {
    let node_client = Api::v1Node(client);
    // Get me a node
    let node = node_client
        .get(NODE_NAME)
        .expect("a real program would handle this error");

    //let mut new_conditions: Vec<NodeCondition> = vec![];

    let mut conditions = node
        .clone()
        .status
        .unwrap_or_else(NodeStatus::default)
        .conditions
        .unwrap_or_else(|| vec![]);
    conditions.iter_mut().for_each(|mut item| {
        item.last_heartbeat_time = Some(Time(Utc::now()));
    });

    // Deep clone, strategically patching as we go.
    let new_node = KubeNode {
        status: Some(NodeStatus {
            conditions: Some(conditions),
            ..node.status.unwrap_or_else(NodeStatus::default)
        }),
        ..node
    };

    info!("{}", serde_json::to_string_pretty(&new_node).unwrap());

    let pp = PatchParams::default();
    let node_data = serde_json::to_vec(&new_node).expect("this to work");
    match node_client.patch(NODE_NAME, &pp, node_data) {
        Ok(_) => println!("updated node"),
        Err(e) => println!("failed to update node: {}", e),
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
    let res = match (req.method(), path[1] /*, path_len*/) {
        (&Method::GET, "containerLogs") => get_container_logs(req),
        (&Method::POST, "exec") => post_exec(req),
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
    Response::new(Body::from(""))
}
