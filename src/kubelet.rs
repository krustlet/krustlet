/// This library contans the Kubelet shell. Use this to create a new Kubelet
/// with a specific handler. (The handler included here is the WASM handler.)
use crate::{
    pod::KubePod,
    server::start_webserver,
    node::{create_node, update_node},
};
use kube::{
    api::{Api, Informer, WatchEvent},
    client::APIClient,
    config::Configuration,
};
use log::{debug, error};
use std::sync::{Arc, Mutex};

#[derive(Fail, Debug)]
#[fail(display = "Operation not supported")]
pub struct NotImplementedError;

/// Describe the lifecycle phase of a workload.
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
    pub fn start(&self, address: std::net::SocketAddr) -> Result<(), failure::Error> {
        let client = APIClient::new(self.kubeconfig.clone());
        // Create the node. If it already exists, "adopt" the node definition
        create_node(client.clone());

        // Start updating the node lease periodically
        let update_client = client.clone();
        let node_updater = std::thread::spawn(move || {
            let sleep_interval = std::time::Duration::from_secs(10);
            loop {
                update_node(update_client.clone());
                std::thread::sleep(sleep_interval);
            }
        });

        // This informer listens for pod events.
        let provider_clone = self.provider.clone();
        let config_clone = self.kubeconfig.clone();
        let ns = self.namespace.clone();
        let pod_informer = std::thread::spawn(move || {
            let pod_client = Api::v1Pod(client.clone()).within(ns.as_str());

            // Create our informer and start listening.
            let informer = Informer::new(pod_client)
                .init()
                .expect("informer init failed");
            loop {
                informer.poll().expect("informer poll failed");
                while let Some(event) = informer.pop() {
                    let config = config_clone.clone();
                    match provider_clone.lock().unwrap().handle_event(event, config) {
                        Ok(_) => debug!("Handled event successfully"),
                        Err(e) => error!("Error handling event: {}", e),
                    };
                }
            }
        });

        // Start the webserver
        start_webserver(self.provider.clone(), &address);

        // Join the threads
        // FIXME: If any of these dies, we should crash the Kubelet and let it restart.
        node_updater.join().expect("node update thread crashed");
        pod_informer.join().expect("informer thread crashed");
        Ok(())
    }
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
    fn logs(&self, _pod: KubePod) -> Result<Vec<String>, failure::Error> {
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
}
