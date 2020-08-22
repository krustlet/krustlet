use std::collections::HashMap;
use std::sync::Arc;

use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Meta, WatchEvent};
use kube::Client as KubeClient;
use log::{debug, error, info};
use tokio::sync::watch;

use crate::pod::{pod_key, Pod};
use crate::provider::Provider;
use crate::state::run_to_completion;

/// Possible mutations to Pod that state machine should detect.
pub enum PodChange {
    /// A container image has changed.
    ImageChange,
    /// The pod was marked for deletion.
    Shutdown,
    /// The pod was deregistered with the Kubernetes API and should be cleaned up.
    Delete,
}

/// A per-pod queue that takes incoming Kubernetes events and broadcasts them to the correct queue
/// for that pod.
///
/// It will also send a error out on the given sender that can be handled in another process (namely
/// the main kubelet process). This queue will only handle the latest update. So if a modify comes
/// in while it is still handling a create and then another modify comes in after, only the second
/// modify will be handled, which is ok given that each event contains the whole pod object
pub(crate) struct Queue<P> {
    provider: Arc<P>,
    handlers: HashMap<String, watch::Sender<WatchEvent<KubePod>>>,
    client: KubeClient,
}

impl<P: 'static + Provider + Sync + Send> Queue<P> {
    pub fn new(provider: Arc<P>, client: KubeClient) -> Self {
        Queue {
            provider,
            handlers: HashMap::new(),
            client,
        }
    }

    async fn run_pod(
        &self,
        initial_event: WatchEvent<KubePod>,
    ) -> anyhow::Result<watch::Sender<WatchEvent<KubePod>>> {
        let (sender, mut receiver) = watch::channel(initial_event);

        // These should be set on the first Add.
        let mut pod_definition: Option<KubePod> = None;
        let mut state_tx: Option<tokio::sync::mpsc::Sender<PodChange>> = None;

        let task_provider = Arc::clone(&self.provider);
        let task_client = self.client.clone();
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                // Watch errors are handled before an event ever gets here, so it should always have
                // a pod
                match event {
                    WatchEvent::Added(pod) => {
                        // TODO Can we avoid having this called multiple times (multiple state machines)?.
                        // I'm thinking we wait for an initial Pod added event before this loop to set
                        // pod_definition and state_tx and start state machine, and after that it is handled differently.

                        let (tx, state_rx) = tokio::sync::mpsc::channel::<PodChange>(16);
                        state_tx = Some(tx);
                        pod_definition = Some(pod.clone());

                        let client = task_client.clone();
                        let pod_state: P::PodState =
                            task_provider.initialize_pod_state().await.unwrap();
                        tokio::spawn(async move {
                            let state: P::InitialState = Default::default();
                            let pod = Pod::new(pod);
                            let name = pod.name().to_string();
                            match run_to_completion(client, state, pod_state, pod, state_rx).await {
                                Ok(()) => info!("Pod {} state machine exited without error", name),
                                Err(e) => {
                                    error!("Pod {} state machine exited with error: {:?}", name, e)
                                }
                            }
                        });
                    }
                    WatchEvent::Modified(_pod) => {
                        // TODO Need to actually detect what change happens. Some kind of diffing functions? Can reference and update pod_definition.
                        match state_tx {
                            Some(ref mut sender) => match sender.send(PodChange::Shutdown).await {
                                Ok(_) => (),
                                // This should only happen if the state machine has completed and rx was dropped.
                                Err(_) => break,
                            },
                            None => unimplemented!(),
                        }
                    }
                    WatchEvent::Deleted(_pod) => match state_tx {
                        Some(ref mut sender) => match sender.send(PodChange::Delete).await {
                            Ok(_) => (),
                            Err(_) => break,
                        },
                        None => unimplemented!(),
                    },
                    WatchEvent::Bookmark(_) => (),
                    _ => unreachable!(),
                }
            }
        });
        Ok(sender)
    }

    pub async fn enqueue(&mut self, event: WatchEvent<KubePod>) -> anyhow::Result<()> {
        match &event {
            WatchEvent::Added(pod)
            | WatchEvent::Bookmark(pod)
            | WatchEvent::Deleted(pod)
            | WatchEvent::Modified(pod) => {
                let pod_name = pod.name();
                let pod_namespace = pod.namespace().unwrap_or_default();
                let key = pod_key(&pod_namespace, &pod_name);
                // We are explicitly not using the entry api here to insert to avoid the need for a
                // mutex
                let sender = match self.handlers.get(&key) {
                    Some(s) => s,
                    None => {
                        self.handlers.insert(
                            key.clone(),
                            // TODO Do we want to capture join handles? Worker wasnt using them.
                            // TODO Does this mean we handle the Add event twice?
                            self.run_pod(event.clone()).await?,
                        );
                        self.handlers.get(&key).unwrap()
                    }
                };
                match sender.broadcast(event) {
                    Ok(_) => debug!(
                        "successfully sent event to handler for pod {} in namespace {}",
                        pod_name, pod_namespace
                    ),
                    Err(e) => error!(
                        "error while sending event. Will retry on next event: {:?}",
                        e
                    ),
                }
                Ok(())
            }
            WatchEvent::Error(e) => Err(e.clone().into()),
        }
    }
}
