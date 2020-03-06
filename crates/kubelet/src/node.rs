use chrono::prelude::*;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::Node;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::{
    api::{Api, PatchParams, PostParams},
    client::APIClient,
};
use log::{debug, error, info};

/// The default node name.
const NODE_NAME: &str = "krustlet";

//type KubeNode = Object<NodeSpec, NodeStatus>;

/// Create a node
///
/// This creates a Kubernetes Node that describes our Kubelet, failing with a log message
/// if one already exists. If one does exist, we simply re-use it. You may call that
/// hacky, but I call it... hacky.
///
/// A node comes with a lease, and we maintain the lease to tell Kubernetes that the
/// node remains alive and functional. Note that this will not work in
/// versions of Kubernetes prior to 1.14.
pub async fn create_node(client: &APIClient, arch: &str) {
    let node_client: Api<Node> = Api::all(client.clone());
    let node = node_definition(arch);

    match node_client
        .create(
            &PostParams::default(),
            serde_json::to_vec(&node).expect("node serializes correctly"),
        )
        .await
    {
        Ok(node) => {
            info!("created node just fine");
            let node_uid = node.metadata.unwrap_or_default().uid.unwrap_or_default();
            create_lease(&node_uid, &client).await
        }
        Err(e) => {
            error!("Error creating node: {}", e);
            info!("Looking up node to see if it exists already");
            match node_client.get(NODE_NAME).await {
                Ok(node) => {
                    let node_uid = node.metadata.unwrap_or_default().uid.unwrap_or_default();
                    create_lease(&node_uid, &client).await
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
pub async fn update_node(client: &APIClient) {
    let node_client: Api<Node> = Api::all(client.clone());
    // Get me a node
    let node_res = node_client.get(NODE_NAME).await;
    match node_res {
        Err(e) => {
            error!("Failed to get node: {:?}", e);
            return;
        }
        Ok(node) => {
            debug!("no error");
            let uid = node.metadata.unwrap_or_default().uid.unwrap_or_default();
            update_lease(&uid, client).await;
        }
    }
}

/// Create a node lease
///
/// These creates a new node lease and claims the node for a set
/// preiod of time. Leases work by creating a new Lease object
/// and then using an ownerReference to tie it to a particular node.
///
/// As far as I can tell, leases ALWAYS go in the 'kube-node-lease'
/// namespace, no exceptions.
async fn create_lease(node_uid: &str, client: &APIClient) {
    let leases: Api<Lease> = Api::namespaced(client.clone(), "kube-node-lease");

    let lease = lease_definition(node_uid);
    let lease_data =
        serde_json::to_vec(&lease).expect("Lease should always be serializable to JSON");
    debug!("{}", serde_json::to_string_pretty(&lease).unwrap());

    let resp = leases.create(&PostParams::default(), lease_data).await;
    match resp {
        Ok(_) => debug!("Created lease"),
        Err(e) => error!("Failed to create lease: {}", e),
    }
}

/// Update the Kubernetes node lease, essentially requesting that we keep
/// the lease for another period.
///
/// TODO: Our patch is overzealous right now. We just need to update the
/// timestamp.
async fn update_lease(node_uid: &str, client: &APIClient) {
    let leases: Api<Lease> = Api::namespaced(client.clone(), "kube-node-lease");

    let lease = lease_definition(node_uid);
    let pp = PatchParams::default();
    let lease_data =
        serde_json::to_vec(&lease).expect("Lease should always be serializable to JSON");
    // TODO: either wrap this in a conditional or remove
    debug!("{}", serde_json::to_string_pretty(&lease).unwrap());

    let resp = leases.patch(NODE_NAME, &pp, lease_data).await;
    match resp {
        Ok(_) => info!("Created lease"),
        Err(e) => error!("Failed to create lease: {}", e),
    }
}

/// Define a new node that will handle WASM load.
///
/// The most important part of this spec is the set of labels, which control
/// how pods are scheduled on this node. It claims the wasm-wasi architecture,
/// though perhaps this should be wasm32-wasi. I am not clear what to do with
/// the OS field. I have seen 'emscripten' used for this field, but in our case
/// the runtime is not emscripten, and besides... specifying which runtime we
/// use seems like a misstep. Ideally, we'll be able to support multiple runtimes.
///
/// TODO: A lot of the values here are faked, and should be replaced by real
/// numbers post-POC.
fn node_definition(arch: &str) -> serde_json::Value {
    let pod_ip = "10.21.77.2";
    let port = 3000;
    let ts = Time(Utc::now());
    json!({
        "apiVersion": "v1",
        "kind": "Node",
        "metadata": {
            "name": NODE_NAME,
            "labels": {
                "beta.kubernetes.io/arch": arch,
                "beta.kubernetes.io/os": "linux",
                "kubernetes.io/arch": arch,
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

/// Define a new coordination.Lease object for Kubernetes
///
/// The lease tells Kubernetes that we want to claim the node for a while
/// longer. And then tells Kubernetes how long it should wait before
/// expecting a new lease.
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
///
/// We set the lease times, the lease duration, and the node name.
fn lease_spec_definition() -> serde_json::Value {
    // Workaround for https://github.com/deislabs/krustlet/issues/5
    // In the future, use LeaseSpec rather than a JSON value
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    json!(
        {
            "holderIdentity": NODE_NAME,
            "acquireTime": now,
            "renewTime": now,
            "leaseDurationSeconds": 300
        }
    )
}
