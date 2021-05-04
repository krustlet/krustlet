//! Defines types for registring controllers with runtime.

#[cfg(feature = "admission-webhook")]
use crate::admission::WebhookFn;
use crate::operator::Operator;
use crate::store::Store;
use futures::FutureExt;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::Metadata;
use kube::api::{GroupVersionKind, ListParams, Resource};
use kube_runtime::watcher::Event;
use std::future::Future;
use std::sync::Arc;
use tracing::{info, warn};

/// Captures configuration needed to configure a watcher.
#[derive(Clone, Debug)]
struct Watch {
    /// The (group, version, kind) tuple of the resource to be watched.
    gvk: GroupVersionKind,
    /// Optionally restrict watching to namespace.
    namespace: Option<String>,
    /// Restrict to objects matching list params (default watches everything).
    list_params: ListParams,
}

impl Watch {
    fn new<
        R: Resource + serde::de::DeserializeOwned + Clone + Metadata<Ty = ObjectMeta> + Send + 'static,
    >(
        namespace: Option<String>,
        list_params: ListParams,
    ) -> Self {
        let gvk = GroupVersionKind::gvk(R::GROUP, R::VERSION, R::KIND).unwrap();
        Watch {
            gvk,
            namespace,
            list_params,
        }
    }

    fn handle(self) -> (WatchHandle, tokio::sync::mpsc::Receiver<DynamicEvent>) {
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let handle = WatchHandle { watch: self, tx };
        (handle, rx)
    }
}

/// Builder pattern for registering a controller or operator.
pub struct ControllerBuilder<C: Operator> {
    /// The controller or operator singleton.
    controller: C,
    ///  List of watch configurations for objects that will simply be cached
    ///  locally.
    watches: Vec<Watch>,
    /// List of watch configurations for objects that will trigger
    /// notifications (based on OwnerReferences).
    owns: Vec<Watch>,
    /// Restrict our controller to act on a specific namespace.
    namespace: Option<String>,
    /// Restrict our controller to act on objects that match specific list
    /// params.
    list_params: ListParams,
}

impl<C: Operator> ControllerBuilder<C>
where
    C::Manifest: Metadata<Ty = ObjectMeta>,
{
    /// Create builder from operator singleton.
    pub fn new(controller: C) -> Self {
        ControllerBuilder {
            controller,
            watches: vec![],
            owns: vec![],
            namespace: None,
            list_params: Default::default(),
        }
    }

    /// Create watcher definition for the configured managed resource.
    fn manages(&self) -> Watch {
        Watch::new::<C::Manifest>(self.namespace.clone(), self.list_params.clone())
    }

    /// Restrict controller to manage a specific namespace.
    pub fn namespaced(mut self, namespace: &str) -> Self {
        self.namespace = Some(namespace.to_string());
        self
    }

    /// Restrict controller to manage only objects matching specific list
    /// params.
    pub fn with_params(mut self, list_params: ListParams) -> Self {
        self.list_params = list_params;
        self
    }

    /// Watch all objects of given kind R. Cluster scoped and no list param
    /// restrictions.
    pub fn watches<R>(mut self) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.watches.push(Watch::new::<R>(None, Default::default()));
        self
    }

    /// Watch objects of given kind R. Cluster scoped, but limited to objects
    /// matching supplied list params.
    pub fn watches_with_params<R>(mut self, list_params: ListParams) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.watches.push(Watch::new::<R>(None, list_params));
        self
    }

    /// Watch all objects of given kind R in supplied namespace, with no list
    /// param restrictions.
    pub fn watches_namespaced<R>(mut self, namespace: &str) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.watches.push(Watch::new::<R>(
            Some(namespace.to_string()),
            Default::default(),
        ));
        self
    }

    /// Watch objects of given kind R in supplied namespace, and limited to
    /// objects matching supplied list params.
    pub fn watches_namespaced_with_params<R>(
        mut self,
        namespace: &str,
        list_params: ListParams,
    ) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.watches
            .push(Watch::new::<R>(Some(namespace.to_string()), list_params));
        self
    }

    /// Watch and subscribe to notifications based on OwnerReferences all
    /// objects of kind R. Cluster scoped and no list param restrictions.
    pub fn owns<R>(mut self) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.owns.push(Watch::new::<R>(None, Default::default()));
        self
    }

    /// Watch and subscribe to notifications based on OwnerReferences
    /// objects of kind R. Cluster scoped, but limited to objects matching
    /// supplied list params.
    pub fn owns_with_params<R>(mut self, list_params: ListParams) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.owns.push(Watch::new::<R>(None, list_params));
        self
    }

    /// Watch and subscribe to notifications based on OwnerReferences
    /// objects of kind R in supplied namespace, with no list param
    /// restrictions.
    pub fn owns_namespaced<R>(mut self, namespace: &str) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.owns.push(Watch::new::<R>(
            Some(namespace.to_string()),
            Default::default(),
        ));
        self
    }

    /// Watch and subscribe to notifications based on OwnerReferences
    /// objects of kind R in supplied namespace, and limited to objects
    /// matching supplied list params.
    pub fn owns_namespaced_with_params<R>(
        mut self,
        namespace: &str,
        list_params: ListParams,
    ) -> Self
    where
        R: Resource
            + serde::de::DeserializeOwned
            + Clone
            + Metadata<Ty = ObjectMeta>
            + Send
            + 'static,
    {
        self.owns
            .push(Watch::new::<R>(Some(namespace.to_string()), list_params));
        self
    }

    /// Registers a validating webhook at the path "/$GROUP/$VERSION/$KIND".
    /// Multiple webhooks can be registered, but must be at different paths.
    #[cfg(feature = "admission-webhook")]
    pub fn validates(mut self, _f: &WebhookFn<C>) -> Self {
        todo!()
    }

    /// Registers a validating webhook at the supplied path.
    #[cfg(feature = "admission-webhook")]
    pub fn validates_at_path(mut self, _path: &str, _f: &WebhookFn<C>) -> Self {
        todo!()
    }

    /// Registers a mutating webhook at the path "/$GROUP/$VERSION/$KIND".
    /// Multiple webhooks can be registered, but must be at different paths.
    #[cfg(feature = "admission-webhook")]
    pub fn mutates(mut self, _f: &WebhookFn<C>) -> Self {
        todo!()
    }

    /// Registers a mutating webhook at the supplied path.
    #[cfg(feature = "admission-webhook")]
    pub fn mutates_at_path(mut self, _path: &str, _f: &WebhookFn<C>) -> Self {
        todo!()
    }
}

#[derive(Clone)]
struct WatchHandle {
    watch: Watch,
    tx: tokio::sync::mpsc::Sender<DynamicEvent>,
}

#[derive(Clone)]
struct Controller {
    manages: WatchHandle,
    owns: Vec<WatchHandle>,
    watches: Vec<WatchHandle>,
}

type OperatorTask = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

async fn launch_runtime<O: Operator>(
    kubeconfig: kube::Config,
    controller: O,
    mut rx: tokio::sync::mpsc::Receiver<DynamicEvent>,
) {
    let mut runtime = crate::OperatorRuntime::new(&kubeconfig, controller, Default::default());
    while let Some(dynamic_event) = rx.recv().await {
        let event: Event<O::Manifest> = dynamic_event.into();
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

async fn launch_watcher(client: kube::Client, handle: WatchHandle) {
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
            Ok(Some(event)) => handle.tx.send(event.into()).await.unwrap(),
            Ok(None) => break,
            Err(error) => warn!(?error, "Error streaming object events."),
        }
    }
}

#[derive(Debug)]
struct DynamicObject {
    name: String,
    namespace: Option<String>,
    data: Box<dyn std::any::Any + Sync + Send>,
}

#[derive(Debug)]
enum DynamicEvent {
    Applied {
        object: DynamicObject,
    },
    Deleted {
        name: String,
        namespace: Option<String>,
        object: DynamicObject,
    },
    Restarted {
        objects: Vec<DynamicObject>,
    },
}

impl<R: kube::Resource + 'static + Sync + Send> From<R> for DynamicObject {
    fn from(object: R) -> Self {
        DynamicObject {
            name: object.name(),
            namespace: object.namespace(),
            data: Box::new(object),
        }
    }
}

impl<R: kube::Resource + 'static + Sync + Send> From<DynamicEvent> for Event<R> {
    fn from(event: DynamicEvent) -> Self {
        match event {
            DynamicEvent::Applied { object } => match object.data.downcast::<R>() {
                Ok(object) => Event::Applied(*object),
                Err(e) => panic!("{:?}", e),
            },
            DynamicEvent::Deleted { object, .. } => match object.data.downcast::<R>() {
                Ok(object) => Event::Applied(*object),
                Err(e) => panic!("{:?}", e),
            },
            DynamicEvent::Restarted {
                objects: dynamic_objects,
            } => {
                let mut objects: Vec<R> = Vec::new();
                for object in dynamic_objects {
                    match object.data.downcast::<R>() {
                        Ok(object) => objects.push(*object),
                        Err(e) => panic!("{:?}", e),
                    }
                }
                Event::Restarted(objects)
            }
        }
    }
}

impl<R: kube::Resource + 'static + Sync + Send> From<Event<R>> for DynamicEvent {
    fn from(event: Event<R>) -> Self {
        match event {
            Event::Applied(object) => DynamicEvent::Applied {
                object: object.into(),
            },
            Event::Deleted(object) => DynamicEvent::Deleted {
                name: object.name(),
                namespace: object.namespace(),
                object: object.into(),
            },
            Event::Restarted(objects) => {
                let mut dynamic_objects = Vec::with_capacity(objects.len());
                for object in objects {
                    dynamic_objects.push(object.into());
                }
                DynamicEvent::Restarted {
                    objects: dynamic_objects,
                }
            }
        }
    }
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
