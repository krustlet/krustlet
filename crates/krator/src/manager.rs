//! Defines types for registring controllers with runtime.
use crate::operator::Operator;
use crate::runtime::PrettyEvent;
use crate::store::Store;
use futures::FutureExt;
use kube::api::DynamicObject;
use kube::api::GroupVersionKind;
use kube::Resource;
use kube_runtime::watcher::Event;
use serde::de::DeserializeOwned;
use std::future::Future;
use std::sync::Arc;
use tracing::{info, warn};

mod controller;
use controller::Controller;
pub use controller::ControllerBuilder;
mod watch;
use watch::launch_watcher;

type OperatorTask = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

fn concrete_object<R>(dynamic_object: DynamicObject) -> anyhow::Result<R>
where
    R: DeserializeOwned,
{
    // TODO: This sucks
    let manifest = serde_json::to_string(&dynamic_object)?;
    Ok(serde_json::from_str::<R>(&manifest)?)
}

fn concrete_event<R>(dynamic_event: Event<DynamicObject>) -> anyhow::Result<Event<R>>
where
    R: DeserializeOwned,
{
    match dynamic_event {
        Event::Applied(dynamic_object) => Ok(Event::Applied(concrete_object(dynamic_object)?)),
        Event::Deleted(dynamic_object) => Ok(Event::Deleted(concrete_object(dynamic_object)?)),
        Event::Restarted(dynamic_objects) => Ok(Event::Restarted(
            dynamic_objects
                .into_iter()
                .map(concrete_object)
                .collect::<anyhow::Result<Vec<R>>>()?,
        )),
    }
}

async fn launch_runtime<O: Operator>(
    kubeconfig: kube::Config,
    controller: O,
    mut rx: tokio::sync::mpsc::Receiver<Event<DynamicObject>>,
) {
    info!(
        group = &*O::Manifest::group(&()),
        version = &*O::Manifest::version(&()),
        kind = &*O::Manifest::kind(&()),
        "Starting OperatorRuntime."
    );
    let mut runtime = crate::OperatorRuntime::new(&kubeconfig, controller, Default::default());
    while let Some(dynamic_event) = rx.recv().await {
        info!(
            group=&*O::Manifest::group(&()),
            version=&*O::Manifest::version(&()),
            kind=&*O::Manifest::kind(&()),
            event = ?PrettyEvent::from(&dynamic_event),
            "Handling managed event."
        );

        match concrete_event::<O::Manifest>(dynamic_event.clone()) {
            Ok(event) => runtime.handle_event(event).await,
            Err(e) => {
                warn!(
                    group=&*O::Manifest::group(&()),
                    version=&*O::Manifest::version(&()),
                    kind=&*O::Manifest::kind(&()),
                    error=?e,
                    "Error deserializing dynamic object: {:#?}", dynamic_event
                );
            }
        }
    }
    warn!(
        group = &*O::Manifest::group(&()),
        version = &*O::Manifest::version(&()),
        kind = &*O::Manifest::kind(&()),
        "Managed Sender dropped."
    );
}

async fn launch_watches(
    mut rx: tokio::sync::mpsc::Receiver<Event<DynamicObject>>,
    gvk: GroupVersionKind,
    store: Arc<Store>,
) {
    while let Some(dynamic_event) = rx.recv().await {
        match dynamic_event {
            Event::Applied(dynamic_object) => {
                let namespace = dynamic_object.metadata.namespace.clone();
                let name = dynamic_object
                    .metadata
                    .name
                    .clone()
                    .expect("Object without name.");
                store
                    .insert_any(namespace, name, &gvk, dynamic_object)
                    .await;
            }
            Event::Deleted(dynamic_object) => {
                let namespace = dynamic_object.metadata.namespace.clone();
                let name = dynamic_object
                    .metadata
                    .name
                    .clone()
                    .expect("Object without name.");
                store.delete_any(namespace, name, &gvk).await;
            }
            Event::Restarted(dynamic_objects) => {
                store.reset(&gvk).await;
                for dynamic_object in dynamic_objects {
                    let namespace = dynamic_object.metadata.namespace.clone();
                    let name = dynamic_object
                        .metadata
                        .name
                        .clone()
                        .expect("Object without name.");
                    store
                        .insert_any(namespace, name, &gvk, dynamic_object)
                        .await;
                }
            }
        }
    }
}

fn launch_controller<C: Operator>(
    kubeconfig: kube::Config,
    controller: ControllerBuilder<C>,
    store: Arc<Store>,
) -> (Controller, Vec<OperatorTask>) {
    let mut watches = Vec::new();
    let mut owns = Vec::new();
    let mut tasks = Vec::new();

    // Create main Operator task.
    let (manages, rx) = controller.manages().handle();
    let task = launch_runtime(kubeconfig, controller.controller, rx).boxed();
    tasks.push(task);

    for watch in controller.watches {
        let (handle, rx) = watch.handle();
        let task = launch_watches(rx, handle.watch.gvk.clone(), Arc::clone(&store)).boxed();
        watches.push(handle);
        tasks.push(task);
    }

    // TODO: This will eventually spawn notification tasks.
    for own in controller.owns {
        let (handle, rx) = own.handle();
        let task = launch_watches(rx, handle.watch.gvk.clone(), Arc::clone(&store)).boxed();
        owns.push(handle);
        tasks.push(task);
    }

    (
        Controller {
            manages,
            owns,
            watches,
        },
        tasks,
    )
}

/// Coordinates one or more controllers and the main entrypoint for starting
/// the application.
// #[derive(Default)]
pub struct Manager {
    kubeconfig: kube::Config,
    controllers: Vec<Controller>,
    controller_tasks: Vec<OperatorTask>,
    store: Arc<Store>,
}

impl Manager {
    /// Create a new controller manager.
    pub fn new(kubeconfig: &kube::Config) -> Self {
        Manager {
            controllers: vec![],
            controller_tasks: vec![],
            kubeconfig: kubeconfig.clone(),
            store: Arc::new(Store::new()),
        }
    }

    /// Register a controller with the manager.
    pub fn register_controller<C: Operator>(&mut self, builder: ControllerBuilder<C>) {
        let (controller, tasks) =
            launch_controller(self.kubeconfig.clone(), builder, Arc::clone(&self.store));
        self.controllers.push(controller);
        self.controller_tasks.extend(tasks);
    }

    /// Start the manager, blocking forever.
    pub async fn start(self) {
        use std::convert::TryFrom;

        let mut tasks = self.controller_tasks;
        let client = kube::Client::try_from(self.kubeconfig)
            .expect("Unable to create kube::Client from kubeconfig.");

        // TODO: Deduplicate Watchers
        for controller in self.controllers {
            tasks.push(launch_watcher(client.clone(), controller.manages).boxed());
            for handle in controller.owns {
                tasks.push(launch_watcher(client.clone(), handle).boxed());
            }
            for handle in controller.watches {
                tasks.push(launch_watcher(client.clone(), handle).boxed());
            }
        }

        futures::future::join_all(tasks).await;
    }
}
