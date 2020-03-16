/// This library contains the Kubelet shell. Use this to create a new Kubelet
/// with a specific handler. (The handler included here is the WASM handler.)
use crate::{
    config::Config,
    node::{create_node, update_node},
    pod::Pod,
    server::start_webserver,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::ContainerStatus as KubeContainerStatus;
use k8s_openapi::api::core::v1::{
    ConfigMap, Container, ContainerState, ContainerStateRunning, ContainerStateTerminated,
    ContainerStateWaiting, EnvVar, Secret,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::{
    api::{Api, ListParams, WatchEvent},
    client::APIClient,
    runtime::Informer,
    Resource,
};
use log::{debug, error, info};
use thiserror::Error;
use tokio::sync::Mutex;

use std::collections::HashMap;
use std::sync::Arc;

#[derive(Error, Debug)]
#[error("Operation not supported")]
pub struct NotImplementedError;

/// Describe the lifecycle phase of a workload.
///
/// This is specified by Kubernetes itself.
#[derive(Clone)]
pub enum Phase {
    /// The workload is currently executing.
    Running,
    /// The workload has exited with an error.
    Failed,
    /// The workload has exited without error.
    Succeeded,
    /// The lifecycle phase of the workload cannot be determined.
    Unknown,
}

/// Describe the status of a workload.
///
/// Phase captures the lifecycle aspect of the workload, while
/// the message provides a human-readable description of the
/// state of the workload.
#[derive(Clone)]
pub struct Status {
    pub phase: Phase,
    pub message: Option<String>,
    pub container_statuses: Vec<ContainerStatus>,
}

/// ContainerStatus is a simplified version of the Kubernetes container status
/// for use in providers. It allows for simple creation of the current status of
/// a "container" (a running wasm process) without worrying about a bunch of
/// Options. Use the [ContainerStatus::to_kubernetes] method for converting it
/// to a Kubernetes API status
#[derive(Clone, Debug)]
pub enum ContainerStatus {
    Waiting {
        /// The timestamp of when this status was reported
        timestamp: DateTime<Utc>,
        /// A human readable string describing the why it is in a waiting status
        message: String,
    },
    Running {
        /// The timestamp of when this status was reported
        timestamp: DateTime<Utc>,
    },
    Terminated {
        /// The timestamp of when this status was reported
        timestamp: DateTime<Utc>,
        /// A human readable string describing the why it is in a terminating status
        message: String,
        /// Should be set to true if the process exited with an error
        failed: bool,
    },
}

impl ContainerStatus {
    pub fn to_kubernetes(&self, pod_name: String) -> KubeContainerStatus {
        let mut state = ContainerState::default();
        match self {
            Self::Waiting { message, .. } => {
                state.waiting.replace(ContainerStateWaiting {
                    message: Some(message.clone()),
                    ..Default::default()
                });
            }
            Self::Running { timestamp } => {
                state.running.replace(ContainerStateRunning {
                    started_at: Some(Time(*timestamp)),
                });
            }
            Self::Terminated {
                timestamp,
                message,
                failed,
            } => {
                state.terminated.replace(ContainerStateTerminated {
                    finished_at: Some(Time(*timestamp)),
                    message: Some(message.clone()),
                    exit_code: *failed as i32,
                    ..Default::default()
                });
            }
        };
        let ready = state.running.is_some();
        KubeContainerStatus {
            state: Some(state),
            name: pod_name,
            // Right now we don't have a way to probe, so just set to ready if
            // in a running state
            ready,
            // This is always true if startupProbe is not defined. When we
            // handle probes, this should be updated accordingly
            started: Some(true),
            // The rest of the items in status (see docs here:
            // https://kubernetes.io/docs/reference/generated/kubernetes-api/v1.17/#containerstatus-v1-core)
            // either don't matter for us or we have not implemented the
            // functionality yet
            ..Default::default()
        }
    }
}

/// Kubelet provides the core Kubelet capability.
///
/// A Kubelet is a special kind of server that handles Kubernetes requests
/// to schedule pods.
///
/// The Kubelet creates a listener on the Kubernetes API (called an Informer),
/// a webserver for API callbacks, and a periodic updater to let Kubernetes
/// know that the node is still running.
///
/// The Provider supplies all of the backend-specific logic. Krustlet will only
/// run one (instance of a) Provider. So a provider may be passed around from
/// thread to thread during the course of the Kubelet's lifetime.
#[derive(Clone)]
pub struct Kubelet<P: 'static + Provider + Clone + Send + Sync> {
    provider: Arc<Mutex<P>>,
    kubeconfig: kube::config::Configuration,
    config: Config,
}

impl<T: 'static + Provider + Sync + Send + Clone> Kubelet<T> {
    /// Create a new Kubelet with a provider, a KubeConfig, and a namespace.
    pub fn new(provider: T, kubeconfig: kube::config::Configuration, config: Config) -> Self {
        Kubelet {
            provider: Arc::new(Mutex::new(provider)),
            kubeconfig,
            config,
        }
    }

    /// Begin answering requests for the Kubelet.
    ///
    /// This will listen on the given address, and will also begin watching for Pod
    /// events, which it will handle.
    pub async fn start(&self) -> anyhow::Result<()> {
        self.provider.lock().await.init().await?;
        let client = APIClient::new(self.kubeconfig.clone());
        // Create the node. If it already exists, "adopt" the node definition
        let conf = self.config.clone();
        let arch = self.provider.lock().await.arch();
        // Get the node name for use in the update loop
        let node_name = conf.node_name.clone();
        create_node(&client, conf, &arch).await;

        // Start updating the node lease periodically
        let update_client = client.clone();
        let node_updater = tokio::task::spawn(async move {
            let sleep_interval = std::time::Duration::from_secs(10);
            loop {
                update_node(&update_client, &node_name).await;
                tokio::time::delay_for(sleep_interval).await;
            }
        });

        // This informer listens for pod events.
        let provider = self.provider.clone();
        let config = self.kubeconfig.clone();

        let pod_informer = tokio::task::spawn(async move {
            // Create our informer and start listening.
            let informer = Informer::new(client, ListParams::default(), Resource::all::<Pod>());
            loop {
                let mut stream = informer.poll().await.expect("informer poll failed").boxed();
                while let Some(event) = stream.try_next().await.unwrap() {
                    match provider
                        .lock()
                        .await
                        .handle_event(event, config.clone())
                        .await
                    {
                        Ok(_) => debug!("Handled event successfully"),
                        Err(e) => error!("Error handling event: {}", e),
                    };
                }
            }
        });

        // Start the webserver
        start_webserver(self.provider.clone(), &self.config.server_config).await?;

        // FIXME: If any of these dies, we should crash the Kubelet and let it restart.
        node_updater.await.expect("node update thread crashed");
        pod_informer.await.expect("informer thread crashed");
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("cannot find pod {}", pod_name)]
    PodNotFound { pod_name: String },
    #[error("cannot find container {} in pod {}", container_name, pod_name)]
    ContainerNotFound {
        pod_name: String,
        container_name: String,
    },
}

/// Provider implements the back-end for the Kubelet.
///
/// The primary responsibility of a Provider is to execut a workload (or schedule it on an external executor)
/// and then monitor it, exposing details back upwards into the Kubelet.
///
/// In most cases, a Provider will not need to directly interact with Kubernetes at all.
/// That is the responsibility of the Kubelet. However, we pass in the client to facilitate
/// cases where a provider may be middleware for another Kubernetes object, or where a
/// provider may require supplemental Kubernetes objects such as Secrets, ConfigMaps, or CRDs.
#[async_trait]
pub trait Provider {
    /// Init is guaranteed to be called only once, and prior to the first call to can_schedule().
    async fn init(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Arch should return a string specifying what architecture this provider supports
    // TODO: Perhaps we need a NodeConfig or other struct that a Provider should return instead
    fn arch(&self) -> String;

    /// Given a Pod definition, this function determines whether or not the workload is schedulable on this provider.
    ///
    /// This determines _only_ if the pod, as described, meets the node requirements (e.g. the node selector).
    /// It is not responsible for determining whether the underlying provider has resources to schedule.
    /// That happens later when `add()` is called.
    ///
    /// It is paramount that this function be fast, as every newly created Pod will come through this
    /// function.
    fn can_schedule(&self, pod: &Pod) -> bool;

    /// Given a Pod definition, execute the workload.
    async fn add(&self, pod: Pod, client: APIClient) -> anyhow::Result<()>;

    /// Given an updated Pod definition, update the given workload.
    ///
    /// Pods that are sent to this function have already met certain criteria for modification.
    /// For example, updates to the `status` of a Pod will not be sent into this function.
    async fn modify(&self, pod: Pod, client: APIClient) -> anyhow::Result<()>;

    /// Given a pod, determine the status of the underlying workload.
    ///
    /// This information is used to update Kubernetes about whether this workload is running,
    /// has already finished running, or has failed.
    async fn status(&self, pod: Pod, client: APIClient) -> anyhow::Result<Status>;

    /// Given the definition of a deleted Pod, remove the workload from the runtime.
    ///
    /// This does not need to actually delete the Pod definition -- just destroy the
    /// associated workload. The default implementation simply returns Ok.
    async fn delete(&self, _pod: Pod, _client: APIClient) -> anyhow::Result<()> {
        Ok(())
    }

    /// Given a Pod, get back the logs for the associated workload.
    ///
    /// The default implementation of this returns a message that this feature is
    /// not available. Override this only when there is an implementation available.
    async fn logs(
        &self,
        _namespace: String,
        _pod: String,
        _container: String,
    ) -> anyhow::Result<Vec<u8>> {
        Err(NotImplementedError {}.into())
    }

    /// Execute a given command on a workload and then return the result.
    ///
    /// The default implementation of this returns a message that this feature is
    /// not available. Override this only when there is an implementation.
    async fn exec(
        &self,
        _pod: Pod,
        _client: APIClient,
        _command: String,
    ) -> anyhow::Result<Vec<String>> {
        Err(NotImplementedError {}.into())
    }

    /// Determine what to do when a new event comes in.
    ///
    /// In most cases, this should not be overridden. It is exposed for rare cases when
    /// the underlying event handling needs to change.
    async fn handle_event(
        &self,
        event: WatchEvent<Pod>,
        config: kube::config::Configuration,
    ) -> anyhow::Result<()> {
        // TODO: Is there value in keeping one client and cloning it?
        let client = APIClient::new(config);
        //let provider = self.provider.clone(); // Arc +1
        match event {
            WatchEvent::Added(p) => {
                // Step 1: Is this legit?
                // Step 2: Can the provider handle this?
                if !self.can_schedule(&p) {
                    debug!(
                        "Provider cannot schedule {}",
                        p.metadata.unwrap_or_default().name.unwrap_or_default()
                    );
                    return Ok(());
                };
                // Step 3: DO IT!
                self.add(p, client).await
            }
            WatchEvent::Modified(p) => {
                // Step 1: Can the provider handle this? (This should be the faster function,
                // so we can weed out negatives quickly.)
                if !self.can_schedule(&p) {
                    debug!(
                        "Provider cannot schedule {}",
                        p.metadata.unwrap_or_default().name.unwrap_or_default()
                    );
                    return Ok(());
                };
                // Step 2: Is this a real modification, or just status?
                // Step 3: DO IT!
                self.modify(p, client).await
            }
            WatchEvent::Deleted(p) => {
                // Step 1: Can the provider handle this?
                if !self.can_schedule(&p) {
                    debug!(
                        "Provider cannot schedule {}",
                        p.metadata.unwrap_or_default().name.unwrap_or_default()
                    );
                    return Ok(());
                };
                // Step 2: DO IT!
                self.delete(p, client).await
            }
            WatchEvent::Error(e) => {
                error!("Event error: {}", e);
                Err(e.into())
            }
        }
    }

    /// Resolve the environment variables for a container.
    ///
    /// This generally should not be overwritten unless you need to handle
    /// environment variable resolution in a special way, such as allowing
    /// custom Downward API fields.
    ///
    /// It is safe to call from within your own providers.
    ///
    /// TODO: Finish secrets, configmaps, and resource fields
    async fn env_vars(
        &self,
        client: APIClient,
        container: &Container,
        pod: &Pod,
    ) -> HashMap<String, String> {
        let mut env = HashMap::new();
        let ns = pod
            .metadata
            .as_ref()
            .and_then(|s| s.namespace.as_deref())
            .unwrap_or("default");
        let empty = Vec::new();
        for env_var in container.env.as_ref().unwrap_or_else(|| &empty).iter() {
            let key = env_var.name.clone();
            let value = match env_var.value.clone() {
                Some(v) => v,
                None => on_missing_value(client.clone(), env_var, ns, &field_map(pod)).await,
            };
            env.insert(key, value);
        }
        env
    }
}

async fn on_missing_value(
    client: APIClient,
    env_var: &EnvVar,
    ns: &str,
    fields: &HashMap<String, String>,
) -> String {
    if let Some(env_src) = env_var.value_from.as_ref() {
        // ConfigMaps
        if let Some(cfkey) = env_src.config_map_key_ref.as_ref() {
            let name = cfkey.name.as_deref().unwrap_or_default();
            match Api::<ConfigMap>::namespaced(client, ns).get(name).await {
                Ok(cfgmap) => {
                    // I am not totally clear on what the outcome should
                    // be of a cfgmap key miss. So for now just return an
                    // empty default.
                    return cfgmap
                        .data
                        .unwrap_or_default()
                        .get(&cfkey.key)
                        .cloned()
                        .unwrap_or_default();
                }
                Err(e) => {
                    error!("Error fetching config map {}: {}", name, e);
                    return "".to_string();
                }
            }
        }
        // Secrets
        if let Some(seckey) = env_src.secret_key_ref.as_ref() {
            let name = seckey.name.as_deref().unwrap_or_default();
            match Api::<Secret>::namespaced(client, ns).get(name).await {
                Ok(secret) => {
                    // I am not totally clear on what the outcome should
                    // be of a secret key miss. So for now just return an
                    // empty default.
                    return secret
                        .data
                        .unwrap_or_default()
                        .remove(&seckey.key)
                        .map(|s| String::from_utf8(s.0).unwrap_or_default())
                        .unwrap_or_default();
                }
                Err(e) => {
                    error!("Error fetching secret {}: {}", name, e);
                    return "".to_string();
                }
            }
        }
        // Downward API (Field Refs)
        if let Some(cfkey) = env_src.field_ref.as_ref() {
            return fields.get(&cfkey.field_path).cloned().unwrap_or_default();
        }
        // Reource Fields (Not implementable just yet... need more of a model.)
    }
    "".to_string()
}

/// Build the map of allowable field_ref values.
///
/// The Downward API only supports a small selection of fields. This
/// provides those fields.
fn field_map(pod: &Pod) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert(
        "metadata.name".into(),
        pod.metadata
            .clone()
            .unwrap_or_default()
            .name
            .unwrap_or_default(),
    );
    map.insert(
        "metadata.namespace".into(),
        pod.metadata
            .clone()
            .unwrap_or_default()
            .namespace
            .unwrap_or_else(|| "default".into()),
    );
    map.insert(
        "spec.serviceAccountName".into(),
        pod.spec
            .clone()
            .unwrap_or_default()
            .service_account_name
            .unwrap_or_default(),
    );
    map.insert(
        "status.hostIP".into(),
        pod.status
            .as_ref()
            .expect("spec must be set")
            .host_ip
            .clone()
            .unwrap_or_default(),
    );
    map.insert(
        "status.podIP".into(),
        pod.status
            .as_ref()
            .expect("spec must be set")
            .pod_ip
            .clone()
            .unwrap_or_default(),
    );
    pod.metadata
        .clone()
        .unwrap_or_default()
        .labels
        .unwrap_or_default()
        .iter()
        .for_each(|(k, v)| {
            info!("adding {} to labels", k);
            map.insert(format!("metadata.labels.{}", k), v.into());
        });
    pod.metadata
        .clone()
        .unwrap_or_default()
        .annotations
        .unwrap_or_default()
        .iter()
        .for_each(|(k, v)| {
            map.insert(format!("metadata.annotations.{}", k), v.into());
        });
    map
}

#[cfg(test)]
mod test {
    use super::*;
    use k8s_openapi::api::core::v1::{
        EnvVar, EnvVarSource, ObjectFieldSelector, PodSpec, PodStatus,
    };
    use kube::api::ObjectMeta;
    use kube::client::APIClient;
    use std::collections::BTreeMap;

    fn mock_client() -> APIClient {
        APIClient::new(kube::config::Configuration {
            base_path: ".".to_string(),
            client: reqwest::Client::new(),
            default_ns: " ".to_string(),
        })
    }

    struct MockProvider {}

    // We use a constructor so that as we update the tests, we don't
    // have to modify a bunch of struct literals with base mock data.
    impl MockProvider {
        fn new() -> Self {
            MockProvider {}
        }
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        fn can_schedule(&self, _pod: &Pod) -> bool {
            true
        }
        fn arch(&self) -> String {
            "mock".to_string()
        }
        async fn add(&self, _pod: Pod, _client: APIClient) -> anyhow::Result<()> {
            Ok(())
        }
        async fn modify(&self, _pod: Pod, _client: APIClient) -> anyhow::Result<()> {
            Ok(())
        }
        async fn status(&self, _pod: Pod, _client: APIClient) -> anyhow::Result<Status> {
            Ok(Status {
                phase: Phase::Succeeded,
                message: None,
                container_statuses: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn test_env_vars() {
        let container = Container {
            env: Some(vec![
                EnvVar {
                    name: "first".into(),
                    value: Some("value".into()),
                    ..Default::default()
                },
                EnvVar {
                    name: "second".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.labels.label".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "third".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.annotations.annotation".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "NAME".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.name".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "NAMESPACE".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.namespace".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "HOST_IP".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "status.hostIP".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "POD_IP".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "status.podIP".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let name = "my-name".to_string();
        let namespace = Some("my-namespace".to_string());
        let mut labels = BTreeMap::new();
        labels.insert("label".to_string(), "value".to_string());
        let mut annotations = BTreeMap::new();
        annotations.insert("annotation".to_string(), "value".to_string());
        let pod = Pod {
            metadata: Some(ObjectMeta {
                labels: Some(labels),
                annotations: Some(annotations),
                name: Some(name),
                namespace,
                ..Default::default()
            }),
            spec: Some(PodSpec {
                service_account_name: Some("svc".to_string()),
                ..Default::default()
            }),
            status: Some(PodStatus {
                host_ip: Some("10.21.77.1".to_string()),
                pod_ip: Some("10.21.77.2".to_string()),
                ..Default::default()
            }),
        };
        let prov = MockProvider::new();
        let env = prov.env_vars(mock_client(), &container, &pod).await;

        assert_eq!(
            "value",
            env.get("first").expect("key first should exist").as_str()
        );

        assert_eq!(
            "value",
            env.get("second").expect("metadata.labels.label").as_str()
        );
        assert_eq!(
            "value",
            env.get("third")
                .expect("metadata.annotations.annotation")
                .as_str()
        );
        assert_eq!("my-name", env.get("NAME").expect("metadata.name").as_str());
        assert_eq!(
            "my-namespace",
            env.get("NAMESPACE").expect("metadata.namespace").as_str()
        );
        assert_eq!("10.21.77.2", env.get("POD_IP").expect("pod_ip").as_str());
        assert_eq!("10.21.77.1", env.get("HOST_IP").expect("host_ip").as_str());
    }
}
