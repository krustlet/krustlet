use chrono::prelude::*;
use k8s_openapi::api::coordination::v1::LeaseSpec;
//use k8s_openapi::api::core::v1::{NodeSpec, NodeStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, Time};
use kube::{
    api::{Api, PatchParams, PostParams, RawApi},
    client::APIClient,
};
use log::{error, info};

/// The default node name.
const NODE_NAME: &str = "krustlet";

//type KubeNode = Object<NodeSpec, NodeStatus>;

/// Create a node
///
/// This creates a Kubernetes Node that describes our Kubelet, failing with a log message
/// if one already exists. If one does exist, we simply re-use it. You may call that
/// hacky, but I call it... hacky.
pub fn create_node(client: APIClient) {
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


/// Update the timestamps on the Node object.
///
/// This is how we report liveness to the upstream.
///
/// We trap errors because... well... quite frankly there is nothing useful
/// to do if the Kubernetes API is unavailable, and we can merrily continue
/// doing our processing of the pod queue.
pub fn udpate_node(client: APIClient) {
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
    let uid = node.metadata.uid;
    update_lease(uid.unwrap_or_else(|| "".to_string()).as_str(), client)
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
