//! Defines common `async` tasks used by Krator's Controller
//! [Manager](crate::manager::Manager).

use std::future::Future;

use futures::FutureExt;

use kube::{api::GroupVersionKind, Resource};
use kube_runtime::watcher::Event;
use tracing::{debug, info, warn};

use crate::{
    manager::controller::ControllerBuilder,
    operator::Operator,
    store::Store,
    util::{concrete_event, DynamicEvent, PrettyEvent},
};

use super::watch::WatchHandle;
use super::Controller;

/// Watcher task which forwards [DynamicEvent](crate::util::DynamicEvent) to
/// a [channel](tokio::sync::mpsc::channel).
pub(crate) async fn launch_watcher(client: kube::Client, handle: WatchHandle) {
    use futures::StreamExt;
    use futures::TryStreamExt;

    info!(
        watch=?handle.watch,
        "Starting Watcher."
    );
    let api: kube::Api<kube::api::DynamicObject> = match handle.watch.namespace {
        Some(namespace) => kube::Api::namespaced_with(client, &namespace, &handle.watch.gvk),
        None => kube::Api::all_with(client, &handle.watch.gvk),
    };
    let mut watcher = kube_runtime::watcher(api, handle.watch.list_params).boxed();
    loop {
        match watcher.try_next().await {
            Ok(Some(event)) => {
                debug!(
                    event = ?PrettyEvent::from(&event),
                    "Handling event."
                );
                handle.tx.send(event).await.unwrap()
            }
            Ok(None) => break,
            Err(error) => warn!(?error, "Error streaming object events."),
        }
    }
}

/// Task for executing a single Controller / Operator. Listens for
/// [DynamicEvent](crate::util::DynamicEvent) on a
/// [channel](tokio::sync::mpsc::channel) and forwards them to a Krator
/// [OperatorRuntime](crate::OperatorRuntime).
///
/// # Errors
///
/// A warning will be logged if a `DynamicEvent` cannot be converted to a
/// concrete `Event<O::Manifest>`.
async fn launch_runtime<O: Operator>(
    kubeconfig: kube::Config,
    controller: O,
    mut rx: tokio::sync::mpsc::Receiver<DynamicEvent>,
    store: Store,
) {
    info!(
        group = &*O::Manifest::group(&()),
        version = &*O::Manifest::version(&()),
        kind = &*O::Manifest::kind(&()),
        "Starting OperatorRuntime."
    );
    let mut runtime =
        crate::OperatorRuntime::new_with_store(&kubeconfig, controller, Default::default(), store);
    while let Some(dynamic_event) = rx.recv().await {
        debug!(
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
async fn launch_watches(
    mut rx: tokio::sync::mpsc::Receiver<DynamicEvent>,
    gvk: GroupVersionKind,
    store: Store,
) {
    while let Some(dynamic_event) = rx.recv().await {
        debug!(
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
                    .insert_gvk(namespace, name, &gvk, dynamic_object)
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
                store.delete_gvk(namespace, name, &gvk).await;
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
                        .insert_gvk(namespace, name, &gvk, dynamic_object)
                        .await;
                }
            }
        }
    }
}

/// Shorthand for the opaque Future type of the tasks in this module. These
/// must be `awaited` in order to execute.
pub(crate) type OperatorTask = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

/// Generates the `async` tasks needed to run a single controller / operator.
///
/// In general, converts a
/// [ControllerBuilder](crate::manager::controller::ControllerBuilder) to a
/// `Vec` of [OperatorTask](crate::manager::tasks::OperatorTask) which can be
/// executed using [join_all](futures::future::join_all).
pub(crate) fn controller_tasks<C: Operator>(
    kubeconfig: kube::Config,
    controller: ControllerBuilder<C>,
    store: Store,
) -> (Controller, Vec<OperatorTask>) {
    let mut watches = Vec::new();
    let mut owns = Vec::new();
    let mut tasks = Vec::new();
    let buffer = controller.buffer();

    // Create main Operator task.
    let (manages, rx) = controller.manages().handle(buffer);
    let task = launch_runtime(kubeconfig, controller.controller, rx, store.clone()).boxed();
    tasks.push(task);

    for watch in controller.watches {
        let (handle, rx) = watch.handle(buffer);
        let task = launch_watches(rx, handle.watch.gvk.clone(), store.clone()).boxed();
        watches.push(handle);
        tasks.push(task);
    }

    for own in controller.owns {
        let (handle, rx) = own.handle(buffer);
        let task = launch_watches(rx, handle.watch.gvk.clone(), store.clone()).boxed();
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
