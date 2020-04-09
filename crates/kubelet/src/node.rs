use crate::config::Config;
use chrono::prelude::*;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::Node;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{Api, DeleteParams, PatchParams, PostParams};
use kube::Error;
use log::{debug, error, info};

macro_rules! retry {
    ($action:expr, times: $num_times:expr, $on_err:expr) => {{
        let mut n = 0u8;
        let mut duration = std::time::Duration::from_millis(100);
        loop {
            n += 1;
            let result = $action;
            match result {
                Ok(_) => break result,
                Err(ref e) => {
                    $on_err(e);
                    tokio::time::delay_for(duration).await;
                    duration *= (n + 1) as u32;
                    if n == $num_times {
                        break result;
                    }
                }
            }
        }
    }};
    ($action:expr, times: $num_times:expr) => {
        retry!($action, times: $num_times, |_| {})
    };
}

/// Create a node
///
/// This creates a Kubernetes Node that describes our Kubelet, failing with a log message
/// if one already exists. If one does exist, we simply re-use it. You may call that
/// hacky, but I call it... hacky.
///
/// A node comes with a lease, and we maintain the lease to tell Kubernetes that the
/// node remains alive and functional. Note that this will not work in
/// versions of Kubernetes prior to 1.14.
pub async fn create_node(client: &kube::Client, config: &Config, arch: &str) {
    let node_client: Api<Node> = Api::all(client.clone());
    let node = node_definition(config, arch);
    let node =
        serde_json::from_value(node).expect("failed to deserialize node from node definition JSON");

    match retry!(node_client.create(&PostParams::default(), &node).await, times: 4) {
        Ok(node) => {
            info!("Successfully created node '{}'", &config.node_name);
            let node_uid = node.metadata.unwrap().uid.unwrap();
            let _create_lease_result =
                retry!(create_lease(&node_uid, &config.node_name, &client).await, times: 4);
        }
        Err(e) => {
            info!(
                "Unable to create node: {:?}, looking up node to see if it exists already...",
                e
            );

            if let Err(e) = retry!(node_client.get(&config.node_name).await, times: 4, |e| info!(
                "Error fetching node after failed create: {}. Retrying...",
                e
            )) {
                error!(
                    "Exhausted retries fetching node after failed create: {}. Not retrying.",
                    e
                )
            }

            info!(
                "Node '{}' found, updating current node definition",
                &config.node_name
            );

            if let Err(e) = retry!(
                replace_node(client, &config.node_name, &node).await,
                times: 4,
                |_| info!(
                    "Node '{}' could not be updated. Retrying...",
                    &config.node_name
                )
            ) {
                error!(
                    "Exhausted retries replacing node after failed create: {}. Not retrying.",
                    e
                )
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
pub async fn update_node(client: &kube::Client, node_name: &str) {
    let node_client: Api<Node> = Api::all(client.clone());
    // Get me a node
    match node_client.get(node_name).await {
        Err(e) => {
            error!("Failed to get node: {:?}", e);
        }
        Ok(node) => {
            let uid = node.metadata.unwrap_or_default().uid.unwrap_or_default();
            update_lease(&uid, node_name, client)
                .await
                .expect("Could not update lease");
        }
    }
}

/// Create a node lease
///
/// These creates a new node lease and claims the node for a set
/// period of time. Leases work by creating a new Lease object
/// and then using an ownerReference to tie it to a particular node.
///
/// As far as I can tell, leases ALWAYS go in the 'kube-node-lease'
/// namespace, no exceptions.
async fn create_lease(
    node_uid: &str,
    node_name: &str,
    client: &kube::Client,
) -> Result<Lease, Error> {
    let leases: Api<Lease> = Api::namespaced(client.clone(), "kube-node-lease");

    let lease = lease_definition(node_uid, node_name);
    let lease = serde_json::from_value(lease)
        .expect("failed to deserialize lease from lease definition JSON");

    let resp = retry!(
        leases.create(&PostParams::default(), &lease).await,
        times: 4,
        |e| info!("Lease could not be created: {}. Retrying...", e)
    );
    match &resp {
        Ok(_) => debug!("Created lease for node '{}'", node_name),
        Err(e) => error!(
            "Exhausted retries creating lease for node '{}': {}",
            node_name, e
        ),
    }
    resp
}

/// Update the Kubernetes node lease, essentially requesting that we keep
/// the lease for another period.
///
/// TODO: Our patch is overzealous right now. We just need to update the
/// timestamp.
async fn update_lease(
    node_uid: &str,
    node_name: &str,
    client: &kube::Client,
) -> Result<Lease, Error> {
    let leases: Api<Lease> = Api::namespaced(client.clone(), "kube-node-lease");

    let lease = lease_definition(node_uid, node_name);
    let lease_data =
        serde_json::to_vec(&lease).expect("Lease should always be serializable to JSON");

    let resp = leases
        .patch(node_name, &PatchParams::default(), lease_data)
        .await;
    match &resp {
        Ok(_) => debug!("Lease updated for '{}'", node_name),
        Err(e) => error!("Failed to update lease for '{}': {}", node_name, e),
    }
    resp
}

async fn replace_node(client: &kube::Client, node_name: &str, node: &Node) -> Result<(), Error> {
    let node_client: Api<Node> = Api::all(client.clone());

    // HACK WARNING: So it turns out we need to have the proper
    // permissions in order to update the node status, so this
    // is a hacky workaround for now where we delete and
    // recreate the node. This is being tracked in https://github.com/deislabs/krustlet/issues/150

    // Delete the node
    retry!(
        node_client
            .delete(node_name, &DeleteParams::default())
            .await,
        times: 4
    )?;
    // Create the node
    let node = retry!(node_client.create(&PostParams::default(), node).await, times: 4)?;
    // Create the lease
    retry!(
        create_lease(
            &node.metadata.clone().unwrap().uid.unwrap(),
            node_name,
            &client,
        )
        .await,
        times: 4
    )?;
    Ok(())
}

/// Define a new node that will handle WASM load.
///
/// The most important part of this spec is the set of labels, which control
/// how pods are scheduled on this node. It claims the wasm-wasi architecture,
/// though perhaps this should be wasm32-wasi. I am not clear what to do with
/// the OS field. I have seen 'emscripten' used for this field, but in our case
/// the runtime is not emscripten, and besides... specifying which runtime we
/// use seems like a misstep. Ideally, we'll be able to support multiple runtimes.
fn node_definition(config: &Config, arch: &str) -> serde_json::Value {
    let ts = Time(Utc::now());
    serde_json::json!({
        "apiVersion": "v1",
        "kind": "Node",
        "metadata": {
            "name": config.node_name,
            "labels": {
                "beta.kubernetes.io/arch": arch,
                "beta.kubernetes.io/os": "linux",
                "kubernetes.io/arch": arch,
                "kubernetes.io/os": "linux",
                "kubernetes.io/hostname": config.hostname,
                "kubernetes.io/role":     "agent",
                "type": "krustlet"
            },
            "annotations": {
                "node.alpha.kubernetes.io/ttl": "0",
                "volumes.kubernetes.io/controller-managed-attach-detach": "true"
            }
        },
        "spec": {
            "podCIDR": "10.244.0.0/24",
            "taints": [
                {
                    "effect": "NoExecute",
                    "key": "krustlet/arch",
                    "value": arch
                }
            ]
        },
        "status": {
            "nodeInfo": {
                "architecture": "wasm-wasi",
                "bootID": "",
                "containerRuntimeVersion": "mvp",
                "kernelVersion": "",
                "kubeProxyVersion": "v1.17.0",
                "kubeletVersion": "v1.17.0",
                "machineID": "",
                "operatingSystem": "linux",
                "osImage": "",
                "systemUUID": ""
            },
            "capacity": {
                "cpu": "4",
                "ephemeral-storage": "61255492Ki",
                "hugepages-1Gi": "0",
                "hugepages-2Mi": "0",
                "memory": "4032800Ki",
                "pods": "30"
            },
            "allocatable": {
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
                    "address": config.node_ip
                },
                {
                    "type": "Hostname",
                    "address": config.hostname
                }
            ],
            "daemonEndpoints": {
                "kubeletEndpoint": {
                    "Port": config.server_config.port
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
fn lease_definition(node_uid: &str, node_name: &str) -> serde_json::Value {
    serde_json::json!(
        {
            "apiVersion": "coordination.k8s.io/v1",
            "kind": "Lease",
            "metadata": {
                "name": node_name,
                "ownerReferences": [
                    {
                        "apiVersion": "v1",
                        "kind": "Node",
                        "name": node_name,
                        "uid": node_uid
                    }
                ]
            },
            "spec": lease_spec_definition(node_name)
        }
    )
}

/// Defines a new coordiation lease for Kubernetes
///
/// We set the lease times, the lease duration, and the node name.
fn lease_spec_definition(node_name: &str) -> serde_json::Value {
    // Workaround for https://github.com/deislabs/krustlet/issues/5
    // In the future, use LeaseSpec rather than a JSON value
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    serde_json::json!(
        {
            "holderIdentity": node_name,
            "acquireTime": now,
            "renewTime": now,
            "leaseDurationSeconds": 300
        }
    )
}
