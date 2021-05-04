//! Defines types for registring controllers with runtime.

#[cfg(feature = "admission-webhook")]
use crate::admission::WebhookFn;
use crate::operator::Operator;
use crate::store::Store;
use futures::FutureExt;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::Metadata;
use kube::api::GroupVersionKind;
use kube_runtime::watcher::Event;
use std::future::Future;
use std::sync::Arc;

mod controller;
use controller::{Controller, ControllerBuilder};
mod watch;
use watch::launch_watcher;
mod dynamic;
use dynamic::DynamicEvent;

type OperatorTask = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

async fn launch_runtime<O: Operator>(
    kubeconfig: kube::Config,
    controller: O,
    mut rx: tokio::sync::mpsc::Receiver<DynamicEvent>,
) {
    use std::convert::TryInto;
    let mut runtime = crate::OperatorRuntime::new(&kubeconfig, controller, Default::default());
    while let Some(dynamic_event) = rx.recv().await {
        let event: Event<O::Manifest> = dynamic_event.try_into().unwrap();
        runtime.handle_event(event).await;
    }
    println!("Sender dropped.");
}

async fn launch_watches(
    mut rx: tokio::sync::mpsc::Receiver<DynamicEvent>,
    gvk: GroupVersionKind,
    store: Arc<Store>,
) {
    while let Some(dynamic_event) = rx.recv().await {
        match dynamic_event {
            DynamicEvent::Applied { object } => {
                store
                    .insert_any(object.namespace, object.name, &gvk, object.data)
                    .await;
            }
            DynamicEvent::Deleted {
                name, namespace, ..
            } => {
                store.delete_any(namespace, name, &gvk).await;
            }
            DynamicEvent::Restarted { objects } => {
                store.reset(&gvk).await;
                for object in objects {
                    store
                        .insert_any(object.namespace, object.name, &gvk, object.data)
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
) -> (Controller, Vec<OperatorTask>)
where
    C::Manifest: Metadata<Ty = ObjectMeta>,
{
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
    pub fn new(kubeconfig: kube::Config) -> Self {
        Manager {
            controllers: vec![],
            controller_tasks: vec![],
            kubeconfig,
            store: Arc::new(Store::new()),
        }
    }

    /// Register a controller with the manager.
    pub fn register_controller<C: Operator>(&mut self, builder: ControllerBuilder<C>)
    where
        C::Manifest: Metadata<Ty = ObjectMeta>,
    {
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
