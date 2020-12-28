use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::{StreamExt, TryStreamExt};
use log::{debug, error, info, warn};
use serde::de::DeserializeOwned;
use tokio::sync::Notify;
use tokio::sync::RwLock;

use kube::{
    api::{Api, ListParams, Meta},
    Client,
};
use kube_runtime::watcher;
use kube_runtime::watcher::Event;

use crate::object::ObjectKey;
use crate::object::{ObjectState, ObjectStatus};
use crate::operator::Operator;
use crate::state::{run_to_completion, SharedState, State};

pub struct OperatorContext<O: Operator> {
    client: Client,
    handlers: HashMap<ObjectKey, tokio::sync::mpsc::Sender<Event<O::Manifest>>>,
    operator: O,
    list_params: ListParams,
}

impl<O: Operator> OperatorContext<O> {
    pub fn new(kubeconfig: &kube::Config, operator: O, params: Option<ListParams>) -> Self {
        let client = Client::new(kubeconfig.clone());
        let list_params = params.unwrap_or_else(Default::default);
        OperatorContext {
            client,
            handlers: HashMap::new(),
            operator,
            list_params,
        }
    }

    /// Dispatch event to the matching resource's task.
    // If no task is found, `self.start_object` is called to start a task for
    // the new object.
    async fn dispatch(&mut self, event: Event<O::Manifest>) -> anyhow::Result<()> {
        match &event {
            Event::Applied(object) => {
                let key: ObjectKey = object.into();
                // We are explicitly not using the entry api here to insert to avoid the need for a
                // mutex
                match self.handlers.get_mut(&key) {
                    Some(sender) => {
                        debug!(
                            "Found existing event handler for object {} in namespace {:?}.",
                            key.name(),
                            key.namespace()
                        );
                        match sender.send(event).await {
                            Ok(_) => debug!(
                                "successfully sent event to handler for object {} in namespace {:?}.",
                                key.name(),
                                key.namespace()
                            ),
                            Err(e) => error!(
                                "error while sending event. Will retry on next event: {:?}.",
                                e
                            ),
                        }
                    }
                    None => {
                        debug!(
                            "Creating event handler for object {} in namespace {:?}.",
                            key.name(),
                            key.namespace()
                        );
                        self.handlers.insert(
                            key.clone(),
                            // TODO Do we want to capture join handles? Worker wasnt using them.
                            // TODO How do we drop this sender / handler?
                            self.start_object(event).await?,
                        );
                    }
                }
                Ok(())
            }
            Event::Deleted(object) => {
                let key: ObjectKey = object.into();
                if let Some(mut sender) = self.handlers.remove(&key) {
                    debug!(
                        "Removed event handler for object {} in namespace {:?}.",
                        key.name(),
                        key.namespace()
                    );
                    sender.send(event).await?;
                }
                Ok(())
            }
            // Restarted should not be passed to this function, it should be passed to resync instead
            Event::Restarted(_) => {
                warn!("Got a restarted event. Restarted events should be resynced with the queue");
                Ok(())
            }
        }
    }

    /// Start task for a single API object.
    // Calls `run_object_task` with first event. Monitors for object deletion
    // on subsequent events.
    async fn start_object(
        &self,
        initial_event: Event<O::Manifest>,
    ) -> anyhow::Result<tokio::sync::mpsc::Sender<Event<O::Manifest>>> {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<Event<O::Manifest>>(16);

        let deleted = Arc::new(Notify::new());

        let shared_manifest = match initial_event {
            Event::Applied(manifest) => {
                let resource_state = self.operator.initialize_resource_state(&manifest).await?;
                let manifest = Arc::new(RwLock::new(manifest));
                tokio::spawn(run_object_task::<
                    O::ObjectState,
                    O::InitialState,
                    O::DeletedState,
                >(
                    self.client.clone(),
                    Arc::clone(&manifest),
                    self.operator.shared_state().await,
                    resource_state,
                    Arc::clone(&deleted),
                ));
                manifest
            }
            _ => return Err(anyhow::anyhow!("Got non-apply event when starting pod")),
        };

        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                // Watch errors are handled before an event ever gets here, so it should always have
                // an object
                match event {
                    Event::Applied(manifest) => {
                        debug!(
                            "Resource {} in namespace {:?} applied.",
                            manifest.name(),
                            manifest.namespace()
                        );
                        let meta = manifest.meta();
                        if meta.deletion_timestamp.is_some() {
                            deleted.notify();
                        }
                        *shared_manifest.write().await = manifest;
                    }
                    Event::Deleted(manifest) => {
                        // I'm not sure if this matters, we get notified of pod deletion with a
                        // Modified event, and I think we only get this after *we* delete the pod.
                        // There is the case where someone force deletes, but we want to go through
                        // our normal terminate and deregister flow anyway.
                        debug!(
                            "Resource {} in namespace {:?} deleted.",
                            manifest.name(),
                            manifest.namespace()
                        );
                        deleted.notify();
                        break;
                    }
                    _ => warn!("Resource got unexpected event, ignoring: {:?}", &event),
                }
            }
        });
        Ok(sender)
    }

    /// Resyncs the queue given the list of objects. Objects that exist in
    /// the queue but no longer exist in the list will be deleted
    async fn resync(&mut self, objects: Vec<O::Manifest>) -> anyhow::Result<()> {
        // First reconcile any deleted items we might have missed (if it exists
        // in our map, but not in the list)
        // TODO
        let _current_objects: HashSet<ObjectKey> = objects.iter().map(|obj| obj.into()).collect();
        let _objects_in_state: HashSet<ObjectKey> = self.handlers.keys().cloned().collect();
        // for key in objects_in_state.difference(&current_objects) {
        //     self.dispatch(Event::Deleted(R {
        //         metadata: ObjectMeta {
        //             name: Some(key.name),
        //             namespace: key.namespace(),
        //             ..Default::default()
        //         },
        //         ..Default::default()
        //     }))
        //     .await?
        // }

        // Now that we've sent off deletes, queue an apply event for all pods
        for object in objects.into_iter() {
            self.dispatch(Event::Applied(object)).await?
        }
        Ok(())
    }

    /// Listens for updates to objects and forwards them to queue.
    pub async fn start(&mut self) {
        let api = Api::<O::Manifest>::all(self.client.clone());
        let mut informer = watcher(api, self.list_params.clone()).boxed();
        loop {
            match informer.try_next().await {
                Ok(Some(event)) => {
                    debug!("Handling Kubernetes object event: {:?}", event);
                    if let Event::Restarted(objects) = event {
                        info!("Got a watch restart. Resyncing queue...");
                        // If we got a restart, we need to requeue an applied event for all objects
                        match self.resync(objects).await {
                            Ok(()) => info!("Finished resync of objects"),
                            Err(e) => warn!("Error resyncing objects: {}", e),
                        };
                    } else {
                        match self.dispatch(event).await {
                            Ok(()) => debug!("Dispatched event for processing"),
                            Err(e) => warn!("Error dispatching object event: {}", e),
                        };
                    }
                }
                Ok(None) => break,
                Err(e) => warn!("Error streaming object events: {:?}", e),
            }
        }
    }
}

async fn run_object_task<
    S: ObjectState,
    InitialState: Default + State<S>,
    DeletedState: Default + State<S>,
>(
    client: Client,
    manifest: Arc<RwLock<S::Manifest>>,
    shared: SharedState<S::SharedState>,
    mut object_state: S,
    deleted: Arc<Notify>,
) where
    S::Manifest: Meta + Clone + DeserializeOwned,
    S::Status: ObjectStatus,
{
    let state: InitialState = Default::default();
    let (namespace, name) = {
        let m = manifest.read().await;
        (m.namespace(), m.name())
    };

    tokio::select! {
        _ = run_to_completion(&client, state, shared.clone(), &mut object_state, Arc::clone(&manifest)) => (),
        _ = deleted.notified() => {
            let state: DeletedState = Default::default();
            debug!("Object {} in namespace {:?} terminated. Jumping to state {:?}.", name, &namespace, state);
            run_to_completion(&client, state, shared.clone(), &mut object_state, Arc::clone(&manifest)).await;
        }
    }

    debug!(
        "Resource {} in namespace {:?} waiting for deregistration.",
        name, namespace
    );
    deleted.notified().await;
    {
        let mut state_writer = shared.write().await;
        object_state.async_drop(&mut state_writer).await;
    }

    let api_client: Api<S::Manifest> = match namespace {
        Some(ref namespace) => kube::Api::namespaced(client, namespace),
        None => kube::Api::all(client),
    };

    let dp = kube::api::DeleteParams {
        grace_period_seconds: Some(0),
        ..Default::default()
    };

    match api_client.delete(&name, &dp).await {
        Ok(_) => {
            debug!(
                "Resource {} in namespace {:?} deregistered.",
                name, namespace
            );
        }
        Err(e) => match e {
            // Ignore not found, already deleted. This could happen if resource was force deleted.
            kube::error::Error::Api(kube::error::ErrorResponse { code, .. }) if code == 404 => (),
            e => {
                warn!(
                    "Unable to deregister resource {} in namespace {:?} with Kubernetes API: {:?}",
                    name, namespace, e
                );
            }
        },
    }
}
