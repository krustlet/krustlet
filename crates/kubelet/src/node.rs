use crate::config::Config;
use chrono::prelude::*;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::Node;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{Api, DeleteParams, PatchParams, PostParams};
use kube::Error;
use log::{debug, error, info};
use std::collections::HashMap;

macro_rules! retry {
    ($action:expr, times: $num_times:expr, error: $on_err:expr) => {{
        let mut n = 0u8;
        let mut duration = std::time::Duration::from_millis(100);
        loop {
            n += 1;
            let result = $action;
            match result {
                Ok(_) => break result,
                Err(ref e) => {
                    if $on_err(e, n) {
                        break result;
                    };
                    tokio::time::delay_for(duration).await;
                    duration *= (n + 1) as u32;
                    if n == $num_times {
                        break result;
                    }
                }
            }
        }
    }};
    ($action:expr, times: $num_times:expr, log_error: $log:expr, break_on: $matches:pat) => {
        retry!($action, times: $num_times, error: |e, _| {
            let matches =  matches!(e, $matches);
            if !matches { $log(e); }
            matches
        })
    };
    ($action:expr, times: $num_times:expr, log_error: $log:expr) => {
        retry!($action, times: $num_times, error: |e, _| { $log(e); false })
    };
    ($action:expr, times: $num_times:expr) => {
        retry!($action, times: $num_times, error: |_, _| { false })
    };
    ($action:expr, times: $num_times:expr, break_on: $matches:pat) => {
        retry!($action, times: $num_times, error: |e, _| { matches!(e, $matches) })
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

    match retry!(node_client.create(&PostParams::default(), &node).await, times: 4, break_on: &Error::Api(kube::ErrorResponse { code: 409, .. }))
    {
        Ok(node) => {
            let node_uid = node.metadata.unwrap().uid.unwrap();
            if let Err(e) = create_lease(&node_uid, &config.node_name, &client).await {
                error!("Failed to create lease: {}", e);
                return;
            }
        }
        Err(Error::Api(kube::ErrorResponse { code: 409, .. })) => {
            debug!(
                "Node '{}' exists already. Going to fetch existing node...",
                &config.node_name
            );

            if let Err(e) = retry!(node_client.get(&config.node_name).await, times: 4, log_error: |e| debug!(
                "Error fetching node after failed create: {}. Retrying...",
                e
            )) {
                error!(
                    "Exhausted retries fetching node after failed create: {}. Not retrying.",
                    e
                );
                return;
            }

            debug!(
                "Node '{}' found, updating current node definition...",
                &config.node_name
            );

            if let Err(e) = replace_node(client, &config.node_name, &node).await {
                error!("Failed to replace node: {}.", e);
                return;
            }
        }
        Err(e) => {
            error!(
                "Exhausted retries creating node after failed create: {}. Not retrying.",
                e
            );
            return;
        }
    };

    info!("Successfully created node '{}'", &config.node_name);
}

/// Update the timestamps on the Node object.
///
/// This is how we report liveness to the upstream.
///
/// We trap errors because... well... quite frankly there is nothing useful
/// to do if the Kubernetes API is unavailable, and we can merrily continue
/// doing our processing of the pod queue.
pub async fn update_node(client: &kube::Client, node_name: &str) {
    debug!("Updating node '{}'", node_name);
    let node_client: Api<Node> = Api::all(client.clone());
    if let Ok(node) = retry!(node_client.get(node_name).await, times: 4, log_error: |e| error!("Failed to get node to update: {:?}", e))
    {
        debug!("Node to update '{}' fetched.", node_name);
        let uid = node.metadata.and_then(|m| m.uid).unwrap();
        retry!(update_lease(&uid, node_name, client).await, times: 4)
            .expect("Could not update lease");
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
async fn create_lease(node_uid: &str, node_name: &str, client: &kube::Client) -> Result<(), Error> {
    debug!("Creating lease for node '{}'", node_name);
    let leases: Api<Lease> = Api::namespaced(client.clone(), "kube-node-lease");

    let lease = lease_definition(node_uid, node_name);
    let lease = serde_json::from_value(lease)
        .expect("failed to deserialize lease from lease definition JSON");

    let resp = retry!(
        leases.create(&PostParams::default(), &lease).await,
        times: 4,
        log_error: |e| debug!("Lease could not be created: {}. Retrying...", e),
        break_on: &Error::Api(kube::ErrorResponse { code: 409, .. })
    );
    match resp {
        Ok(_) => {
            debug!("Created lease for node '{}'", node_name);
            Ok(())
        }
        Err(Error::Api(kube::ErrorResponse { code: 409, .. })) => {
            debug!("Lease already existed for node '{}'", node_name);
            Ok(())
        }
        Err(e) => {
            error!(
                "Exhausted retries creating lease for node '{}': {}",
                node_name, e
            );
            Err(e)
        }
    }
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
    debug!("Updating lease for node '{}'...", node_name);
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
    debug!("Replacing existing node '{}'", node_name);
    let node_client: Api<Node> = Api::all(client.clone());

    // HACK WARNING: So it turns out we need to have the proper
    // permissions in order to update the node status, so this
    // is a hacky workaround for now where we delete and
    // recreate the node. This is being tracked in https://github.com/deislabs/krustlet/issues/150

    // Delete the node
    debug!(
        "Deleting existing node '{}' in order to recreate it",
        node_name
    );
    retry!(
        node_client
            .delete(node_name, &DeleteParams::default())
            .await,
        times: 4,
        log_error: |e| debug!("Could not delete node during replacement: {}", e)
    )?;
    debug!("Recreating recently deleted existing node '{}'", node_name);
    // Create the node
    let node = retry!(node_client.create(&PostParams::default(), node).await, times: 4, log_error: |e| debug!("Could not create node during replacement: {}", e))?;
    // Create the lease
    create_lease(
        &node.metadata.and_then(|m| m.uid).unwrap(),
        node_name,
        &client,
    )
    .await?;

    debug!("Successfully replaced node '{}'", node_name);
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
    let node_labels = node_labels_definition(arch, &config);

    let mut json = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Node",
        "metadata": {
            "name": config.node_name,
            "labels": {},
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
    });

    // extra labels from config
    for (key, val) in node_labels {
        json["metadata"]["labels"][key] = serde_json::json!(val);
    }
    json
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

/// Defines the labels that will be applied to this node
///
/// Default values and passed node-labels arguments are injected by config.
fn node_labels_definition(arch: &str, config: &Config) -> HashMap<String, String> {
    // clone from config should include A) all default values and B) any
    // passed via the --node-labels argument
    let mut labels = config.node_labels.clone();

    // add the mandatory labels that are dependent on injected values
    labels.insert("beta.kubernetes.io/arch".to_string(), arch.to_string());
    labels.insert("kubernetes.io/arch".to_string(), arch.to_string());
    labels.insert(
        "kubernetes.io/hostname".to_string(),
        config.hostname.to_string(),
    );

    labels
}
