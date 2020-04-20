use std::collections::HashMap;
use std::sync::Arc;

use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Meta, WatchEvent};
use log::{debug, error};
use tokio::sync::{mpsc::Sender, watch, Mutex};
use tokio::task::JoinHandle;

use crate::handle::pod_key;
use crate::Provider;

/// A per-pod queue that takes incoming Kubernetes events and broadcasts them to the correct queue
/// for that pod. It will also send a error out on the given sender that can be handled in another
/// process (namely the main kubelet process). This queue will only handle the latest update. So if
/// a modify comes in while it is still handling a create and then another modify comes in after,
/// only the second modify will be handled, which is ok given that each event contains the whole pod
/// object
pub struct PodQueue<P> {
    provider: Arc<P>,
    handlers: Mutex<HashMap<String, Worker>>,
    error_sender: Sender<(KubePod, anyhow::Error)>,
}

struct Worker {
    sender: watch::Sender<WatchEvent<KubePod>>,
    _worker: JoinHandle<()>,
}

impl Worker {
    fn create<P>(
        initial_event: WatchEvent<KubePod>,
        provider: Arc<P>,
        mut error_sender: Sender<(KubePod, anyhow::Error)>,
    ) -> Self
    where
        P: 'static + Provider + Sync + Send,
    {
        let (sender, mut receiver) = watch::channel(initial_event);
        let worker = tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                // Watch errors are handled before an event ever gets here, so it should always have
                // a pod
                let pod = pod_from_event(&event).unwrap();
                if let Err(e) = provider.handle_event(event).await {
                    if let Err(e) = error_sender.send((pod, e)).await {
                        error!("Unable to send error to status updater: {:?}", e)
                    }
                }
            }
        });
        Worker {
            sender,
            _worker: worker,
        }
    }
}

impl<P: 'static + Provider + Sync + Send> PodQueue<P> {
    pub fn new(provider: Arc<P>, error_sender: Sender<(KubePod, anyhow::Error)>) -> Self {
        PodQueue {
            provider,
            handlers: Mutex::new(HashMap::new()),
            error_sender,
        }
    }

    pub async fn enqueue(&mut self, event: WatchEvent<KubePod>) -> anyhow::Result<()> {
        match &event {
            WatchEvent::Added(pod)
            | WatchEvent::Bookmark(pod)
            | WatchEvent::Deleted(pod)
            | WatchEvent::Modified(pod) => {
                let mut handlers = self.handlers.lock().await;
                let pod_name = pod.name();
                let pod_namespace = pod.namespace().unwrap_or_default();
                let entry = handlers
                    .entry(pod_key(&pod_namespace, &pod_name))
                    .or_insert_with(|| {
                        Worker::create(
                            event.clone(),
                            self.provider.clone(),
                            self.error_sender.clone(),
                        )
                    });
                // TODO: There is a possible condition where we get two modifies right in a row and
                // the 1st one locks/broadcasts after the second one. This shouldn't matter too much
                // right now, but we should fix that possibility in the future
                match entry.sender.broadcast(event) {
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

fn pod_from_event(event: &WatchEvent<KubePod>) -> Option<KubePod> {
    match event {
        WatchEvent::Added(pod)
        | WatchEvent::Bookmark(pod)
        | WatchEvent::Deleted(pod)
        | WatchEvent::Modified(pod) => Some(pod.clone()),
        WatchEvent::Error(_) => None,
    }
}
