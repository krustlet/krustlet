//! Defines common `async` tasks used by Krator's Controller
//! [Manager](crate::manager::Manager).

use std::{
    future::Future,
    sync::Arc
};

use futures::FutureExt;

use kube::{
    Resource,
    api::GroupVersionKind
};
use kube_runtime::watcher::Event;
use tracing::{warn, info};

use crate::{
    operator::Operator,
    util::{
        PrettyEvent,
        DynamicEvent,
        concrete_event
    },
    manager::ControllerBuilder,
    store::Store
};

use super::Controller;

/// Task for executing a single Controller / Operator. Listens for
/// [DynamicEvent](crate::util::DynamicEvent) on a
/// [channel](tokio::sync::mpsc::channel) and forwards them to a Krator
/// [OperatorRuntime](crate::OperatorRuntime).
///
/// # Errors
///
/// A warning will be logged if a `DynamicEvent` cannot be converted to a
/// concrete `Event<O::Manifest>`. 
pub async fn launch_runtime<O: Operator>(
    kubeconfig: kube::Config,
    controller: O,
    mut rx: tokio::sync::mpsc::Receiver<DynamicEvent>,
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

/// Task for monitoring `watched` or `owned` resources. Listens for
/// [DynamicEvent](crate::util::DynamicEvent) on a
/// [channel](tokio::sync::mpsc::channel) and updates
/// [Store](crate::store::Store).
/// 
/// # Errors
///
/// Will warn on and drop objects with no `metadata.name` field set.
///
/// # TODO
///
/// * Support notifications for `owned` resources.
pub async fn launch_watches(
    mut rx: tokio::sync::mpsc::Receiver<DynamicEvent>,
    gvk: GroupVersionKind,
    store: Arc<Store>,
) {
    while let Some(dynamic_event) = rx.recv().await {
        info!(
            gvk=?gvk,
            event = ?PrettyEvent::from(&dynamic_event),
            "Handling watched event."
        );
        match dynamic_event {
            Event::Applied(dynamic_object) => {
                let namespace = dynamic_object.metadata.namespace.clone();
                let name = match dynamic_object.metadata.name.clone() {
                    Some(name) => name,
                    None => {
                        warn!(
                            gvk=?gvk,
                            "Object without name."
                        );
                        continue;
                    }
                };
                store
                    .insert_any(namespace, name, &gvk, dynamic_object)
                    .await;
            }
            Event::Deleted(dynamic_object) => {
                let namespace = dynamic_object.metadata.namespace.clone();
                let name = match dynamic_object.metadata.name.clone() {
                    Some(name) => name,
                    None => {
                        warn!(
                            gvk=?gvk,
                            "Object without name."
                        );
                        continue;
                    }
                };
                store.delete_any(namespace, name, &gvk).await;
            }
            Event::Restarted(dynamic_objects) => {
                store.reset(&gvk).await;
                for dynamic_object in dynamic_objects {
                    let namespace = dynamic_object.metadata.namespace.clone();
                    let name = match dynamic_object.metadata.name.clone() {
                        Some(name) => name,
                        None => {
                            warn!(
                                gvk=?gvk,
                                "Object without name."
                            );
                            continue;
                        }
                    };
                    store
                        .insert_any(namespace, name, &gvk, dynamic_object)
                        .await;
                }
            }
        }
    }
}


/// Shorthand for the opaque Future type of the tasks in this module. These
/// must be `awaited` in order to execute.
pub type OperatorTask = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

/// Generates the `async` tasks needed to run a single controller / operator.
///
/// In general, converts a 
/// [ControllerBuilder](crate::manager::ControllerBuilder) to a `Vec` of
/// [OperatorTask](crate::manager::tasks::OperatorTask) which can be
/// executed using [join_all](futures::future::join_all).
pub fn controller_tasks<C: Operator>(
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


