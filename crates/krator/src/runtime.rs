use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::{StreamExt, TryStreamExt};
use tokio::sync::mpsc::Sender;
use tokio::sync::Notify;
use tracing::{debug, error, info, trace, warn};

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::Metadata;
use kube::{
    api::{Api, ListParams, Meta},
    Client,
};
use kube_runtime::watcher;
use kube_runtime::watcher::Event;

use crate::manifest::Manifest;
use crate::object::ObjectKey;
use crate::object::ObjectState;
use crate::operator::Operator;
use crate::state::{run_to_completion, SharedState};

#[derive(Debug)]
enum PrettyEvent {
    Applied {
        name: String,
        namespace: Option<String>,
    },
    Deleted {
        name: String,
        namespace: Option<String>,
    },
    Restarted {
        count: usize,
    },
}

impl<R: Meta> From<&Event<R>> for PrettyEvent {
    fn from(event: &Event<R>) -> Self {
        match event {
            Event::Applied(object) => PrettyEvent::Applied {
                name: object.name(),
                namespace: object.namespace(),
            },
            Event::Deleted(object) => PrettyEvent::Deleted {
                name: object.name(),
                namespace: object.namespace(),
            },
            Event::Restarted(objects) => PrettyEvent::Restarted {
                count: objects.len(),
            },
        }
    }
}

/// Accepts a type implementing the `Operator` trait and watches
/// for resources of the associated `Manifest` type, running the
/// associated state machine for each. Optionally filter by
/// `kube::api::ListParams`.
pub struct OperatorRuntime<O: Operator> {
    client: Client,
    handlers: HashMap<ObjectKey, Sender<Event<O::Manifest>>>,
    operator: Arc<O>,
    list_params: ListParams,
    signal: Option<Arc<AtomicBool>>,
}

impl<O: Operator> OperatorRuntime<O> {
    /// Create new runtime with optional ListParams.
    pub fn new(kubeconfig: &kube::Config, operator: O, params: Option<ListParams>) -> Self {
        let client = Client::try_from(kubeconfig.clone())
            .expect("Unable to create kube::Client from kubeconfig.");
        let list_params = params.unwrap_or_default();
        OperatorRuntime {
            client,
            handlers: HashMap::new(),
            operator: Arc::new(operator),
            list_params,
            signal: None,
        }
    }

    /// Dispatch event to the matching resource's task.
    /// If no task is found, `self.start_object` is called to start a task for
    /// the new object.
    #[tracing::instrument(
      level="trace",
      skip(self, event),
      fields(event = ?PrettyEvent::from(&event))
    )]
    async fn dispatch(&mut self, event: Event<O::Manifest>) -> anyhow::Result<()> {
        match &event {
            Event::Applied(object) => {
                let key: ObjectKey = object.into();
                // We are explicitly not using the entry api here to insert to avoid the need for a
                // mutex
                match self.handlers.get_mut(&key) {
                    Some(sender) => {
                        trace!("Found existing event handler for object.");
                        match sender.send(event).await {
                            Ok(_) => trace!("Successfully sent event to handler for object."),
                            Err(error) => error!(
                                name=key.name(),
                                namespace=?key.namespace(),
                                ?error,
                                "Error while sending event. Will retry on next event.",
                            ),
                        }
                    }
                    None => {
                        debug!(
                            name=key.name(),
                            namespace=?key.namespace(),
                            "Creating event handler for object.",
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
                if let Some(sender) = self.handlers.remove(&key) {
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
    ) -> anyhow::Result<Sender<Event<O::Manifest>>> {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<Event<O::Manifest>>(128);

        let deleted = Arc::new(Notify::new());

        let (manifest, object_state) = match initial_event {
            Event::Applied(manifest) => {
                let object_state = self.operator.initialize_object_state(&manifest).await?;
                (manifest, object_state)
            }
            _ => return Err(anyhow::anyhow!("Got non-apply event when starting pod")),
        };

        let (manifest_tx, manifest_rx) = Manifest::new(manifest);
        let reflector_deleted = Arc::clone(&deleted);

        // Two tasks are spawned for each resource. The first updates shared state (manifest and
        // deleted flag) while the second awaits on the actual state machine, interrupts it on
        // deletion, and handles cleanup.

        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                // Watch errors are handled before an event ever gets here, so it should always have
                // an object
                match event {
                    Event::Applied(manifest) => {
                        trace!(
                            "Resource {} in namespace {:?} applied.",
                            manifest.name(),
                            manifest.namespace()
                        );
                        let meta = manifest.meta();
                        if meta.deletion_timestamp.is_some() {
                            reflector_deleted.notify_one();
                        }
                        match manifest_tx.send(manifest) {
                            Ok(()) => (),
                            Err(_) => {
                                debug!("Manifest receiver hung up, exiting.");
                                return;
                            }
                        }
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
                        reflector_deleted.notify_one();
                        match manifest_tx.send(manifest) {
                            Ok(()) => (),
                            Err(_) => {
                                debug!("Manifest receiver hung up, exiting.");
                                return;
                            }
                        }
                        break;
                    }
                    _ => warn!("Resource got unexpected event, ignoring: {:?}", &event),
                }
            }
        });

        tokio::spawn(run_object_task::<O>(
            self.client.clone(),
            manifest_rx,
            self.operator.shared_state().await,
            object_state,
            deleted,
            Arc::clone(&self.operator),
        ));

        Ok(sender)
    }

    /// Resyncs the queue given the list of objects. Objects that exist in
    /// the queue but no longer exist in the list will be deleted
    #[tracing::instrument(
      level="trace",
      skip(self, objects),
      fields(count=objects.len())
    )]
    async fn resync(&mut self, objects: Vec<O::Manifest>) -> anyhow::Result<()> {
        // First reconcile any deleted items we might have missed (if it exists
        // in our map, but not in the list)
        let current_objects: HashSet<ObjectKey> = objects.iter().map(|obj| obj.into()).collect();
        let objects_in_state: HashSet<ObjectKey> = self.handlers.keys().cloned().collect();
        for key in objects_in_state.difference(&current_objects) {
            let mut manifest: O::Manifest = Default::default();
            {
                let meta: &mut ObjectMeta = manifest.metadata_mut();
                meta.name = Some(key.name().to_string());
                meta.namespace = key.namespace().cloned();
            }
            trace!(
                name=key.name(),
                namespace=?key.namespace(),
                "object_deleted"
            );
            self.dispatch(Event::Deleted(manifest)).await?;
        }

        // Now that we've sent off deletes, queue an apply event for all pods
        for object in objects.into_iter() {
            trace!(
                name=%object.name(),
                namespace=?object.namespace(),
                "object_applied"
            );
            self.dispatch(Event::Applied(object)).await?
        }
        Ok(())
    }

    #[tracing::instrument(
        level="trace",
        skip(self, event),
        fields(event=?PrettyEvent::from(&event))
    )]
    async fn handle_event(&mut self, event: Event<O::Manifest>) {
        if let Some(ref signal) = self.signal {
            if matches!(event, kube_runtime::watcher::Event::Applied(_))
                && signal.load(Ordering::Relaxed)
            {
                warn!("Controller is shutting down (got signal). Dropping Add event.");
                return;
            }
        }
        if let Event::Restarted(objects) = event {
            info!("Got a watch restart. Resyncing queue...");
            // If we got a restart, we need to requeue an applied event for all objects
            match self.resync(objects).await {
                Ok(()) => info!("Finished resync of objects."),
                Err(error) => warn!(?error, "Error resyncing objects."),
            };
        } else {
            match self.dispatch(event).await {
                Ok(()) => debug!("Dispatched event for processing."),
                Err(error) => warn!(?error, "Error dispatching object event."),
            };
        }
    }

    /// Listens for updates to objects and forwards them to queue.
    pub async fn main_loop(&mut self) {
        let api = Api::<O::Manifest>::all(self.client.clone());
        let mut informer = watcher(api, self.list_params.clone()).boxed();
        loop {
            match informer.try_next().await {
                Ok(Some(event)) => self.handle_event(event).await,
                Ok(None) => break,
                Err(error) => warn!(?error, "Error streaming object events."),
            }
        }
    }

    /// Start Operator (blocks forever).
    #[cfg(not(feature = "admission-webhook"))]
    pub async fn start(&mut self) {
        self.main_loop().await;
    }

    /// Start Operator (blocks forever).
    #[cfg(feature = "admission-webhook")]
    pub async fn start(&mut self) {
        let hook = crate::admission::endpoint(Arc::clone(&self.operator));
        let main = self.main_loop();
        tokio::select!(
            _ = main => warn!("Main loop exited"),
            _ = hook => warn!("Admission hook exited."),
        )
    }
}

async fn run_object_task<O: Operator>(
    client: Client,
    manifest: Manifest<O::Manifest>,
    shared: SharedState<<O::ObjectState as ObjectState>::SharedState>,
    mut object_state: O::ObjectState,
    deleted: Arc<Notify>,
    operator: Arc<O>,
) {
    debug!("Running registration hook.");
    let state: O::InitialState = Default::default();
    let (namespace, name) = {
        let m = manifest.latest();
        match operator.registration_hook(manifest.clone()).await {
            Ok(()) => debug!("Running hook complete."),
            Err(e) => {
                error!(
                    "Operator registration hook for object {} in namespace {:?} failed: {:?}",
                    m.name(),
                    m.namespace(),
                    e
                );
                return;
            }
        }
        (m.namespace(), m.name())
    };

    tokio::select! {
        _ = run_to_completion(&client, state, shared.clone(), &mut object_state, manifest.clone()) => (),
        _ = deleted.notified() => {
            let state: O::DeletedState = Default::default();
            debug!("Object {} in namespace {:?} terminated. Jumping to state {:?}.", name, &namespace, state);
            run_to_completion(&client, state, shared.clone(), &mut object_state, manifest.clone()).await;
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

    match operator.deregistration_hook(manifest).await {
        Ok(()) => (),
        Err(e) => warn!(
            "Operator deregistration hook for object {} in namespace {:?} failed: {:?}",
            name, namespace, e
        ),
    }

    let api_client: Api<O::Manifest> = match namespace {
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
