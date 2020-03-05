/// This library contains the Kubelet shell. Use this to create a new Kubelet
/// with a specific handler. (The handler included here is the WASM handler.)
use crate::{
    node::{create_node, update_node},
    pod::KubePod,
    server::start_webserver,
};
use k8s_openapi::api::core::v1::Container;
use kube::{
    api::{Api, Informer, WatchEvent},
    client::APIClient,
    config::Configuration,
};
use log::{debug, error, info};
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

#[derive(Fail, Debug)]
#[fail(display = "Operation not supported")]
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
    kubeconfig: Configuration,
    namespace: String,
}

impl<T: 'static + Provider + Sync + Send + Clone> Kubelet<T> {
    /// Create a new Kubelet with a provider, a KubeConfig, and a namespace.
    pub fn new(provider: T, kubeconfig: Configuration, namespace: String) -> Self {
        Kubelet {
            provider: Arc::new(Mutex::new(provider)),
            kubeconfig,
            namespace,
        }
    }

    /// Begin answering requests for the Kubelet.
    ///
    /// This will listen on the given address, and will also begin watching for Pod
    /// events, which it will handle.
    pub fn start(&self, address: std::net::SocketAddr) -> Result<(), failure::Error> {
        self.provider.lock().unwrap().init()?;
        let client = APIClient::new(self.kubeconfig.clone());
        // Create the node. If it already exists, "adopt" the node definition
        create_node(&client, &self.provider.lock().unwrap().arch());

        // Start updating the node lease periodically
        let update_client = client.clone();
        let node_updater = std::thread::spawn(move || {
            let sleep_interval = std::time::Duration::from_secs(10);
            loop {
                update_node(&update_client);
                std::thread::sleep(sleep_interval);
            }
        });

        // This informer listens for pod events.
        let provider = self.provider.clone();
        let config = self.kubeconfig.clone();

        // TODO: I think this should listen in all namespaces!
        let ns = self.namespace.clone();
        let pod_informer = std::thread::spawn(move || {
            let pod_client = Api::v1Pod(client).within(ns.as_str());

            // Create our informer and start listening.
            let informer = Informer::new(pod_client)
                .init()
                .expect("informer init failed");
            loop {
                informer.poll().expect("informer poll failed");
                while let Some(event) = informer.pop() {
                    // TODO: We need to spawn threads (or do something similar)
                    // to handle the event. Currently, there is only one thread
                    // executing WASM.
                    match provider.lock().unwrap().handle_event(event, config.clone()) {
                        Ok(_) => debug!("Handled event successfully"),
                        Err(e) => error!("Error handling event: {}", e),
                    };
                }
            }
        });

        // Start the webserver
        start_webserver(self.provider.clone(), &address)?;

        // Join the threads
        // FIXME: If any of these dies, we should crash the Kubelet and let it restart.
        node_updater.join().expect("node update thread crashed");
        pod_informer.join().expect("informer thread crashed");
        Ok(())
    }
}

#[derive(Debug, Fail)]
pub enum ProviderError {
    #[fail(display = "cannot find pod {}", pod_name)]
    PodNotFound { pod_name: String },
    #[fail(
        display = "cannot find container {} in pod {}",
        container_name, pod_name
    )]
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
pub trait Provider {
    /// Init is guaranteed to be called only once, and prior to the first call to can_schedule().
    fn init(&self) -> Result<(), failure::Error> {
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
    fn can_schedule(&self, pod: &KubePod) -> bool;

    /// Given a Pod definition, execute the workload.
    fn add(&self, pod: KubePod, client: APIClient) -> Result<(), failure::Error>;

    /// Given an updated Pod definition, update the given workload.
    ///
    /// Pods that are sent to this function have already met certain criteria for modification.
    /// For example, updates to the `status` of a Pod will not be sent into this function.
    fn modify(&self, pod: KubePod, client: APIClient) -> Result<(), failure::Error>;

    /// Given a pod, determine the status of the underlying workload.
    ///
    /// This information is used to update Kubernetes about whether this workload is running,
    /// has already finished running, or has failed.
    fn status(&self, pod: KubePod, client: APIClient) -> Result<Status, failure::Error>;

    /// Given the definition of a deleted Pod, remove the workload from the runtime.
    ///
    /// This does not need to actually delete the Pod definition -- just destroy the
    /// associated workload. The default implementation simply returns Ok.
    fn delete(&self, _pod: KubePod, _client: APIClient) -> Result<(), failure::Error> {
        Ok(())
    }

    /// Given a Pod, get back the logs for the associated workload.
    ///
    /// The default implementation of this returns a message that this feature is
    /// not available. Override this only when there is an implementation available.
    fn logs(
        &self,
        _namespace: String,
        _pod: String,
        _container: String,
    ) -> Result<Vec<String>, failure::Error> {
        Err(NotImplementedError {}.into())
    }

    /// Execute a given command on a workload and then return the result.
    ///
    /// The default implementation of this returns a message that this feature is
    /// not available. Override this only when there is an implementation.
    fn exec(
        &self,
        _pod: KubePod,
        _client: APIClient,
        _command: String,
    ) -> Result<Vec<String>, failure::Error> {
        Err(NotImplementedError {}.into())
    }

    /// Determine what to do when a new event comes in.
    ///
    /// In most cases, this should not be overridden. It is exposed for rare cases when
    /// the underlying event handling needs to change.
    fn handle_event(
        &self,
        event: WatchEvent<KubePod>,
        config: Configuration,
    ) -> Result<(), failure::Error> {
        // TODO: Is there value in keeping one client and cloning it?
        let client = APIClient::new(config);
        //let provider = self.provider.clone(); // Arc +1
        match event {
            WatchEvent::Added(p) => {
                // Step 1: Is this legit?
                // Step 2: Can the provider handle this?
                if !self.can_schedule(&p) {
                    debug!("Provider cannot schedule {}", p.metadata.name);
                    return Ok(());
                };
                // Step 3: DO IT!
                self.add(p, client)
            }
            WatchEvent::Modified(p) => {
                // Step 1: Can the provider handle this? (This should be the faster function,
                // so we can weed out negatives quickly.)
                if !self.can_schedule(&p) {
                    debug!("Provider cannot schedule {}", p.metadata.name);
                    return Ok(());
                };
                // Step 2: Is this a real modification, or just status?
                // Step 3: DO IT!
                self.modify(p, client)
            }
            WatchEvent::Deleted(p) => {
                // Step 1: Can the provider handle this?
                if !self.can_schedule(&p) {
                    debug!("Provider cannot schedule {}", p.metadata.name);
                    return Ok(());
                };
                // Step 2: DO IT!
                self.delete(p, client)
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
    fn env_vars(
        &self,
        client: APIClient,
        container: &Container,
        pod: &KubePod,
    ) -> HashMap<String, String> {
        let fields = field_map(pod);
        let mut env = HashMap::new();
        let empty = Vec::new();
        let ns = pod.metadata.namespace.as_deref().unwrap_or("default");
        container
            .env
            .as_ref()
            .unwrap_or_else(|| &empty)
            .iter()
            .for_each(|i| {
                env.insert(
                    i.name.clone(),
                    i.value.clone().unwrap_or_else(|| {
                        let client = client.clone();
                        if let Some(env_src) = i.value_from.as_ref() {
                            // ConfigMaps
                            if let Some(cfkey) = env_src.config_map_key_ref.as_ref() {
                                let name = cfkey.name.as_deref().unwrap_or("");
                                match Api::v1ConfigMap(client).within(ns).get(name) {
                                    Ok(cfgmap) => {
                                        // I am not totally clear on what the outcome should
                                        // be of a cfgmap key miss. So for now just return an
                                        // empty default.
                                        return cfgmap
                                            .data
                                            .get(cfkey.key.as_str())
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
                                match Api::v1Secret(client).within(ns).get(name) {
                                    Ok(secret) => {
                                        // I am not totally clear on what the outcome should
                                        // be of a cfgmap key miss. So for now just return an
                                        // empty default.

                                        return secret
                                            .stringData
                                            .get(seckey.key.as_str())
                                            .cloned()
                                            .unwrap_or_default();
                                    }
                                    Err(e) => {
                                        error!("Error fetching config map {}: {}", name, e);
                                        return "".to_string();
                                    }
                                }
                            }
                            // Downward API (Field Refs)
                            if let Some(cfkey) = env_src.field_ref.as_ref() {
                                return fields
                                    .get(cfkey.field_path.as_str())
                                    .cloned()
                                    .unwrap_or_default();
                            }
                            // Reource Fields (Not implementable just yet... need more of a model.)
                        }
                        "".to_string()
                    }),
                );
            });
        env
    }
}

/// Build the map of allowable field_ref values.
///
/// The Downward API only supports a small selection of fields. This
/// provides those fields.
fn field_map(pod: &KubePod) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("metadata.name".into(), pod.metadata.name.clone());
    map.insert(
        "metadata.namespace".into(),
        pod.metadata
            .namespace
            .clone()
            .unwrap_or_else(|| "default".into()),
    );
    map.insert(
        "spec.serviceAccountName".into(),
        pod.spec.service_account_name.clone().unwrap_or_default(),
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
    pod.metadata.labels.iter().for_each(|(k, v)| {
        info!("adding {} to labels", k);
        map.insert(format!("metadata.labels.{}", k), v.into());
    });
    pod.metadata.annotations.iter().for_each(|(k, v)| {
        map.insert(format!("metadata.annotations.{}", k), v.into());
    });
    map
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::pod::KubePod;
    use k8s_openapi::api::core::v1::{
        EnvVar, EnvVarSource, ObjectFieldSelector, PodSpec, PodStatus,
    };
    use kube::api::ObjectMeta;
    use kube::client::APIClient;
    use std::collections::BTreeMap;

    fn mock_client() -> APIClient {
        APIClient::new(Configuration {
            base_path: ".".into(),
            client: reqwest::Client::new(),
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

    impl Provider for MockProvider {
        fn can_schedule(&self, _pod: &KubePod) -> bool {
            true
        }
        fn arch(&self) -> String {
            "mock".to_string()
        }
        fn add(&self, _pod: KubePod, _client: APIClient) -> Result<(), failure::Error> {
            Ok(())
        }
        fn modify(&self, _pod: KubePod, _client: APIClient) -> Result<(), failure::Error> {
            Ok(())
        }
        fn status(&self, _pod: KubePod, _client: APIClient) -> Result<Status, failure::Error> {
            Ok(Status {
                phase: Phase::Succeeded,
                message: None,
            })
        }
    }

    #[test]
    fn test_env_vars() {
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
        let pod = KubePod {
            metadata: ObjectMeta {
                labels,
                annotations,
                name,
                namespace,
                ..Default::default()
            },
            spec: PodSpec {
                service_account_name: Some("svc".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                host_ip: Some("10.21.77.1".to_string()),
                pod_ip: Some("10.21.77.2".to_string()),
                ..Default::default()
            }),
            types: Default::default(),
        };
        let prov = MockProvider::new();
        let env = prov.env_vars(mock_client(), &container, &pod);

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
