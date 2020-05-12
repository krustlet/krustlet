use crate::config::Config;
use crate::provider::Provider;
use chrono::prelude::*;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::Node as KubeNode;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{Api, DeleteParams, PatchParams, PostParams};
use kube::error::ErrorResponse;
use kube::Error;
use log::{debug, error, info, warn};
use std::collections::BTreeMap;
use std::sync::Arc;

const KUBELET_VERSION: &'static str = env!("CARGO_PKG_VERSION");

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
pub async fn create_node<P: 'static + Provider + Sync + Send>(
    client: &kube::Client,
    config: &Config,
    provider: Arc<P>,
) {
    let node_client: Api<KubeNode> = Api::all(client.clone());

    let mut builder = Node::builder();

    builder.set_name(&config.node_name);

    builder.add_annotation("node.alpha.kubernetes.io/ttl", "0");
    builder.add_annotation(
        "volumes.kubernetes.io/controller-managed-attach-detach",
        "true",
    );

    node_labels_definition(P::ARCH, &config, &mut builder);

    // TODO Do we want to detect this?
    builder.add_capacity("cpu", "4");
    builder.add_capacity("ephemeral-storage", "61255492Ki");
    builder.add_capacity("hugepages-1Gi", "0");
    builder.add_capacity("hugepages-2Mi", "0");
    builder.add_capacity("memory", "4032800Ki");
    builder.add_capacity("pods", &config.max_pods.to_string());

    builder.add_allocatable("cpu", "4");
    builder.add_allocatable("ephemeral-storage", "61255492Ki");
    builder.add_allocatable("hugepages-1Gi", "0");
    builder.add_allocatable("hugepages-2Mi", "0");
    builder.add_allocatable("memory", "4032800Ki");
    builder.add_allocatable("pods", &config.max_pods.to_string());

    let ts = Utc::now();
    builder.add_condition("Ready", "True", &ts, "KubeletReady", "kubelet is ready");
    builder.add_condition(
        "OutOfDisk",
        "False",
        &ts,
        "KubeletHasSufficientDisk",
        "kubelet has sufficient disk space available",
    );

    builder.add_address("InternalIP", &format!("{}", config.node_ip));
    builder.add_address("Hostname", &config.hostname);

    builder.set_port(config.server_config.port as i32);

    provider.node(&mut builder);
    let node = builder.build().into_inner();

    match retry!(node_client.create(&PostParams::default(), &node).await, times: 4, break_on: &Error::Api(ErrorResponse { code: 409, .. }))
    {
        Ok(node) => {
            let node_uid = node.metadata.unwrap().uid.unwrap();
            if let Err(e) = create_lease(&node_uid, &config.node_name, &client).await {
                error!("Failed to create lease: {}", e);
                return;
            }
        }
        Err(Error::Api(ErrorResponse { code: 409, .. })) => {
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
    let node_client: Api<KubeNode> = Api::all(client.clone());
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
        break_on: &Error::Api(ErrorResponse { code: 409, .. })
    );
    match resp {
        Ok(_) => {
            debug!("Created lease for node '{}'", node_name);
            Ok(())
        }
        Err(Error::Api(ErrorResponse { code: 409, .. })) => {
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

async fn replace_node(
    client: &kube::Client,
    node_name: &str,
    node: &KubeNode,
) -> Result<(), Error> {
    debug!("Replacing existing node '{}'", node_name);
    let node_client: Api<KubeNode> = Api::all(client.clone());

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
fn node_labels_definition(arch: &str, config: &Config, builder: &mut NodeBuilder) {
    // Add mandatory static labels
    builder.add_label("beta.kubernetes.io/os", "linux");
    builder.add_label("kubernetes.io/os", "linux");
    builder.add_label("kubernetes.io/role", "agent");
    builder.add_label("type", "krustlet");
    // add the mandatory labels that are dependent on injected values
    builder.add_label("beta.kubernetes.io/arch", arch);
    builder.add_label("kubernetes.io/arch", arch);
    builder.add_label("kubernetes.io/hostname", &config.hostname);

    let k8s_namespace = "kubernetes.io";
    // namespaces managed by this method - do not allow user injection
    let managed_namespace_labels = [
        "beta.kubernetes.io/arch",
        "beta.kubernetes.io/os",
        "kubernetes.io/arch",
        "kubernetes.io/hostname",
        "kubernetes.io/os",
        "kubernetes.io/role",
        "type",
    ];
    let allowed_k8s_namespace_labels = [
        "beta.kubernetes.io/instance-type",
        "failure-domain.beta.kubernetes.io/region",
        "failure-domain.beta.kubernetes.io/zone",
        "failure-domain.kubernetes.io/region",
        "failure-domain.kubernetes.io/zone",
        "kubernetes.io/instance-type",
    ];

    // Attempt to append node labels from passed arguments.
    // First, check for managed namespace and log exclusion
    // Next, check if label contains k8s namespace and ensure it's allowable
    // Else, if not k8s namspace, insert
    let user_labels = &config.node_labels;

    for (key, value) in user_labels.iter() {
        if managed_namespace_labels.contains(&key.as_str()) {
            warn!(
                "User provided node label {} omitted. Namespace label managed by runtime.",
                key
            );
        } else if key.contains(k8s_namespace)
            && !key.starts_with("kubelet.kubernetes.io")
            && !key.starts_with("node.kubernetes.io")
            && !allowed_k8s_namespace_labels.contains(&key.as_str())
        {
            warn!(
                "User provided node label {} omitted. Namespace violates constraints.",
                key
            );
        } else {
            builder.add_label(key, value);
        }
    }
}

/// Kubernetes Node Definition. Wraps `k8s_openapi::api::core::v1::Node`.
pub struct Node(k8s_openapi::api::core::v1::Node);

impl Node {
    /// Create builder for node definition.
    pub fn builder() -> NodeBuilder {
        Default::default()
    }

    /// Extract inner `k8s_openapi::api::core::v1::Node` object from node definition.
    pub fn into_inner(self) -> KubeNode {
        self.0
    }
}

impl From<KubeNode> for Node {
    /// Create node definition from `k8s_openapi::api::core::v1::Node` object.
    fn from(node: KubeNode) -> Self {
        Node(node)
    }
}

/// Builder for node definition.
pub struct NodeBuilder {
    name: String,
    annotations: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
    pod_cidr: String,
    taints: Vec<k8s_openapi::api::core::v1::Taint>,
    architecture: String,
    kube_proxy_version: String,
    kubelet_version: String,
    container_runtime_version: String,
    operating_system: String,
    capacity: BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>,
    allocatable: BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>,
    port: i32,
    conditions: Vec<k8s_openapi::api::core::v1::NodeCondition>,
    addresses: Vec<k8s_openapi::api::core::v1::NodeAddress>,
}

impl NodeBuilder {
    /// Create new builder with defaults.
    pub fn new() -> Self {
        Default::default()
    }

    /// Add an annotation for the node.
    pub fn add_annotation(&mut self, key: &str, value: &str) {
        self.annotations.insert(key.to_string(), value.to_string());
    }

    /// Add a label to the node.
    pub fn add_label(&mut self, key: &str, value: &str) {
        self.labels.insert(key.to_string(), value.to_string());
    }

    /// Set the name of the node.
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// Sets the CIDR that pods will be assigned IPs from.
    pub fn set_pod_cidr(&mut self, cidr: &str) {
        self.pod_cidr = cidr.to_string();
    }

    /// Add a taint to the node.
    pub fn add_taint(&mut self, effect: &str, key: &str, value: &str) {
        self.taints.push(k8s_openapi::api::core::v1::Taint {
            effect: effect.to_string(),
            key: key.to_string(),
            value: Some(value.to_string()),
            time_added: None,
        });
    }

    /// Set the architecture of the node.
    pub fn set_architecture(&mut self, arch: &str) {
        self.architecture = arch.to_string();
    }

    /// Set the kube proxy version of the node.
    pub fn set_kube_proxy_version(&mut self, version: &str) {
        self.kube_proxy_version = version.to_string();
    }

    /// Set the kubelet version of the node.
    pub fn set_kubelet_version(&mut self, version: &str) {
        self.kubelet_version = version.to_string();
    }

    /// Set the container runtime version of the node.
    pub fn set_container_runtime_version(&mut self, version: &str) {
        self.container_runtime_version = version.to_string();
    }

    /// Set the operating system of the node.
    pub fn set_operating_system(&mut self, os: &str) {
        self.operating_system = os.to_string();
    }

    /// Add a capacity of the node.
    pub fn add_capacity(&mut self, key: &str, value: &str) {
        self.capacity.insert(
            key.to_string(),
            k8s_openapi::apimachinery::pkg::api::resource::Quantity(value.to_string()),
        );
    }

    /// Add an allocatable of the node.
    pub fn add_allocatable(&mut self, key: &str, value: &str) {
        self.allocatable.insert(
            key.to_string(),
            k8s_openapi::apimachinery::pkg::api::resource::Quantity(value.to_string()),
        );
    }

    /// Set the port for the node.
    pub fn set_port(&mut self, port: i32) {
        self.port = port
    }

    /// Add a condition of the node.
    pub fn add_condition(
        &mut self,
        type_: &str,
        status: &str,
        timestamp: &DateTime<Utc>,
        reason: &str,
        message: &str,
    ) {
        self.conditions
            .push(k8s_openapi::api::core::v1::NodeCondition {
                type_: type_.to_string(),
                status: status.to_string(),
                last_heartbeat_time: Some(Time(timestamp.clone())),
                last_transition_time: Some(Time(timestamp.clone())),
                reason: Some(reason.to_string()),
                message: Some(message.to_string()),
            });
    }

    /// Add a address to the node.
    pub fn add_address(&mut self, type_: &str, address: &str) {
        self.addresses
            .push(k8s_openapi::api::core::v1::NodeAddress {
                type_: type_.to_string(),
                address: address.to_string(),
            });
    }

    /// Build node definition from builder.
    pub fn build(self) -> Node {
        let mut metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta =
            Default::default();
        metadata.name = Some(self.name);
        metadata.annotations = Some(self.annotations);
        metadata.labels = Some(self.labels);

        let mut spec: k8s_openapi::api::core::v1::NodeSpec = Default::default();
        spec.pod_cidr = Some(self.pod_cidr);
        spec.taints = Some(self.taints);

        let mut node_info: k8s_openapi::api::core::v1::NodeSystemInfo = Default::default();
        node_info.architecture = self.architecture;
        node_info.kube_proxy_version = self.kube_proxy_version;
        node_info.kubelet_version = self.kubelet_version;
        node_info.container_runtime_version = self.container_runtime_version;
        node_info.operating_system = self.operating_system;

        let mut status: k8s_openapi::api::core::v1::NodeStatus = Default::default();
        status.node_info = Some(node_info);
        status.capacity = Some(self.capacity);
        status.allocatable = Some(self.allocatable);
        status.daemon_endpoints = Some(k8s_openapi::api::core::v1::NodeDaemonEndpoints {
            kubelet_endpoint: Some(k8s_openapi::api::core::v1::DaemonEndpoint { port: self.port }),
        });
        status.conditions = Some(self.conditions);
        status.addresses = Some(self.addresses);

        let kube_node = k8s_openapi::api::core::v1::Node {
            metadata: Some(metadata),
            spec: Some(spec),
            status: Some(status),
        };
        Node(kube_node)
    }
}

impl Default for NodeBuilder {
    fn default() -> NodeBuilder {
        NodeBuilder {
            name: "krustlet".to_string(),
            annotations: BTreeMap::new(),
            labels: BTreeMap::new(),
            pod_cidr: "10.244.0.0/24".to_string(),
            taints: vec![],
            architecture: "".to_string(),
            kube_proxy_version: "v1.17.0".to_string(),
            kubelet_version: KUBELET_VERSION.to_string(),
            container_runtime_version: "mvp".to_string(),
            operating_system: "linux".to_string(),
            capacity: BTreeMap::new(),
            allocatable: BTreeMap::new(),
            port: 10250,
            conditions: vec![],
            addresses: vec![],
        }
    }
}

impl Default for Node {
    fn default() -> Node {
        Node::builder().build()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::{Config, ServerConfig};
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::path::PathBuf;

    #[test]
    fn test_node_labels_definition() {
        let mut node_labels = HashMap::new();
        node_labels.insert("foo".to_owned(), "custom".to_owned());
        node_labels.insert(
            "kubelet.kubernetes.io/allowed-prefix".to_owned(),
            "prefix".to_owned(),
        );
        node_labels.insert(
            "not-allowed.kubernetes.io".to_owned(),
            "not-allowed".to_owned(),
        );
        node_labels.insert(
            "kubernetes.io/instance-type".to_owned(),
            "allowed".to_owned(),
        );
        node_labels.insert("beta.kubernetes.io/os".to_owned(), "managed".to_owned());

        let config = Config {
            node_ip: IpAddr::from(Ipv4Addr::new(127, 0, 0, 1)),
            hostname: String::from("foo"),
            node_name: String::from("bar"),
            server_config: ServerConfig {
                addr: IpAddr::from(Ipv4Addr::new(127, 0, 0, 1)),
                port: 8080,
                pfx_password: String::new(),
                pfx_path: PathBuf::new(),
            },
            data_dir: PathBuf::new(),
            node_labels,
            max_pods: 110,
        };

        let mut builder = Node::builder();
        node_labels_definition("linux", &config, &mut builder);

        let result = builder.labels;

        assert!(result.contains_key("kubernetes.io/role"));
        assert!(result.contains_key("foo"));
        assert!(result.contains_key("kubelet.kubernetes.io/allowed-prefix"));
        assert!(!result.contains_key("not-allowed.kubernetes.io"));
        assert!(result.contains_key("kubernetes.io/instance-type"));
        assert!(!result.get("beta.kubernetes.io/os").unwrap().eq("managed"));
        assert!(result.get("beta.kubernetes.io/os").unwrap().eq("linux"));
    }
}
