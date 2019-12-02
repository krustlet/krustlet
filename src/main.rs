#[macro_use]
extern crate serde_json;
extern crate base64;

use chrono::prelude::*;
use env_logger;
use futures::future;
use hyper::rt::Future;
use hyper::service::service_fn;
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::api::core::v1::{NodeSpec, NodeStatus, PodSpec, PodStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, Time};
use kube::{
    api::{Api, Informer, Object, PatchParams, PostParams, RawApi, WatchEvent},
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
    let node_client = Api::v1Node(client.clone());
    let pp = PostParams::default();
    let node = node_definition();

    match node_client.create(
        &pp,
        serde_json::to_vec(&node).expect("node serializes correctly"),
    ) {
        Ok(node) => {
            info!("created node just fine");
            let node_uid = node.metadata.uid.unwrap_or_else(|| "".to_string());
            create_lease(node_uid.as_str(), client)
        }
        Err(e) => {
            error!("Error creating node: {}", e);
            info!("Looking up node to see if it exists already");
            match node_client.get(NODE_NAME) {
                Ok(node) => {
                    let node_uid = node.metadata.uid.unwrap_or_else(|| "".to_string());
                    create_lease(node_uid.as_str(), client)
                }
                Err(e) => error!("Error fetching node after failed create: {}", e),
            }
        }
    };
}

/// Create a node lease
fn create_lease(node_uid: &str, client: APIClient) {
    let leases = RawApi::customResource("leases")
        .version("v1")
        .group("coordination.k8s.io")
        .within("kube-node-lease"); // Spec says all leases go here

    let lease = lease_definition(node_uid);
    let pp = PostParams::default();
    let lease_data =
        serde_json::to_vec(&lease).expect("Lease should always be serializable to JSON");
    // TODO: either wrap this in a conditional or remove
    info!("{}", serde_json::to_string_pretty(&lease).unwrap());

    let req = leases
        .create(&pp, lease_data)
        .expect("Lease should always convert to a request");
    match client.request::<serde_json::Value>(req) {
        Ok(_) => info!("Created lease"),
        Err(e) => error!("Failed to create lease: {}", e),
    }
}

fn update_lease(node_uid: &str, client: APIClient) {
    let leases = RawApi::customResource("leases")
        .version("v1")
        .group("coordination.k8s.io")
        .within("kube-node-lease"); // Spec says all leases go here

    let lease = lease_definition(node_uid);
    let pp = PatchParams::default();
    let lease_data =
        serde_json::to_vec(&lease).expect("Lease should always be serializable to JSON");
    // TODO: either wrap this in a conditional or remove
    info!("{}", serde_json::to_string_pretty(&lease).unwrap());

    let req = leases
        .patch(NODE_NAME, &pp, lease_data)
        .expect("Lease should always convert to a request");
    match client.request::<serde_json::Value>(req) {
        Ok(_) => info!("Created lease"),
        Err(e) => error!("Failed to create lease: {}", e),
    }
}

fn node_definition() -> serde_json::Value {
    let pod_ip = "10.21.77.2";
    let port = 3000;
    let ts = Time(Utc::now());
    json!({
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
            },
            "alocatable": {
                "cpu": "4",
                "ephemeral-storage": "61255492Ki",
                "hugepages-1Gi": "0",
                "hugepages-2Mi": "0",
                "memory": "4032800Ki",
                "pods": "30"
            },
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "lastHeartbeatTime":  ts,
                    "lastTransitionTime": ts,
                    "reason":             "KubeletReady",
                    "message":            "kubelet is ready",
                },
                {
                    "type": "OutOfDisk",
                    "status": "False",
                    "lastHeartbeatTime":  ts,
                    "lastTransitionTime": ts,
                    "reason":             "KubeletHasSufficientDisk",
                    "message":            "kubelet has sufficient disk space available",
                },
            ],
            "addresses": [
                {
                    "type": "InternalIP",
                    "address": pod_ip
                },
                {
                    "type": "Hostname",
                    "address": "krustlet"
                }
            ],
            "daemonEndpoints": {
                "kubeletEndpoint": {
                    "Port": port
                }
            }
        }
    })
}

fn lease_definition(node_uid: &str) -> serde_json::Value {
    json!(
        {
            "apiVersion": "coordination.k8s.io/v1",
            "kind": "Lease",
            "metadata": {
                "name": NODE_NAME,
                "ownerReferences": [
                    {
                        "apiVersion": "v1",
                        "kind": "Node",
                        "name": NODE_NAME,
                        "uid": node_uid
                    }
                ]
            },
            "spec": lease_spec_definition()
        }
    )
}

/// Defines a new coordiation lease for Kubernetes
fn lease_spec_definition() -> LeaseSpec {
    LeaseSpec {
        holder_identity: Some(NODE_NAME.to_string()),
        acquire_time: Some(MicroTime(Utc::now())),
        renew_time: Some(MicroTime(Utc::now())),
        lease_duration_seconds: Some(300),
        ..Default::default()
    }
}

/// Update the timestamps on the Node object.
///
/// This is how we report liveness to the upstream.
///
/// We trap errors because... well... quite frankly there is nothing useful
/// to do if the Kubernetes API is unavailable, and we can merrily continue
/// doing our processing of the pod queue.
fn udpate_node(client: APIClient) {
    let node_client = Api::v1Node(client.clone());
    // Get me a node
    let node_res = node_client.get(NODE_NAME);
    match node_res {
        Err(e) => {
            error!("Failed to get node: {:?}", e);
            return;
        }
        _ => {
            println!("no error");
        }
    }
    let node = node_res.unwrap();
    let uid = node.metadata.clone().uid;

    /* I don't think this is necessary
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

    println!("{}", serde_json::to_string_pretty(&new_node).unwrap());

    let pp = PatchParams::default();
    let node_data = serde_json::to_vec(&new_node).expect("this to work");
    match node_client.patch(NODE_NAME, &pp, node_data) {
        Ok(res) => {
            info!("updated node");
            println!(
                "updated: {}",
                serde_json::to_string_pretty(&res.status.unwrap()).unwrap()
            )
        }
        Err(e) => error!("failed to update node: {}", e),
    };
    */
    update_lease(uid.unwrap_or_else(|| "".to_string()).as_str(), client)
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
