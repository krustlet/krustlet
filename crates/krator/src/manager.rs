//! Defines types for registring controllers with runtime.

#[cfg(feature = "admission-webhook")]
use crate::admission::WebhookFn;
use crate::operator::Operator;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::Metadata;
use k8s_openapi::Resource;
use kube::api::{GroupVersionKind, ListParams};

/// Captures configuration needed to configure a watcher.
struct Watch {
    /// The (group, version, kind) tuple of the resource to be watched.
    _gvk: GroupVersionKind,
    /// Optionally restrict watching to namespace.
    _namespace: Option<String>,
    /// Restrict to objects matching list params (default watches everything).
    _list_params: ListParams,
}

impl Watch {
    fn new<
        R: Resource + serde::de::DeserializeOwned + Clone + Metadata<Ty = ObjectMeta> + Send + 'static,
    >(
        _client: kube::Client,
        _namespace: Option<String>,
        _list_params: ListParams,
    ) -> Self {
        let _gvk = GroupVersionKind::gvk(R::GROUP, R::VERSION, R::KIND).unwrap();
        Watch {
            _gvk,
            _namespace,
            _list_params,
        }
    }
}

/// Builder pattern for registering a controller or operator.
pub struct ControllerBuilder<C: Operator> {
    /// The controller or operator singleton.
    _controller: C,
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
    /// Used for constructing watchers.
    client: kube::Client,
}

impl<C: Operator> ControllerBuilder<C> {
    /// Create builder from operator singleton.
    pub fn new(client: kube::Client, controller: C) -> Self {
        ControllerBuilder {
            _controller: controller,
            client,
            watches: vec![],
            owns: vec![],
            namespace: None,
            list_params: Default::default(),
        }
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
        self.watches.push(Watch::new::<R>(
            self.client.clone(),
            None,
            Default::default(),
        ));
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
        self.watches
            .push(Watch::new::<R>(self.client.clone(), None, list_params));
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
            self.client.clone(),
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
        self.watches.push(Watch::new::<R>(
            self.client.clone(),
            Some(namespace.to_string()),
            list_params,
        ));
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
        self.owns.push(Watch::new::<R>(
            self.client.clone(),
            None,
            Default::default(),
        ));
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
        self.owns
            .push(Watch::new::<R>(self.client.clone(), None, list_params));
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
            self.client.clone(),
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
        self.owns.push(Watch::new::<R>(
            self.client.clone(),
            Some(namespace.to_string()),
            list_params,
        ));
        self
    }

    /// Registers a validating webhook at the path "/$KIND".
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

    /// Registers a mutating webhook at the path "/$KIND".
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

/// Coordinates one or more controllers and the main entrypoint for starting
/// the application.
#[derive(Default)]
pub struct Manager;

impl Manager {
    /// Create a new controller manager.
    pub fn new() -> Self {
        Manager
    }

    /// Register a controller with the manager.
    pub fn register_controller<C: Operator>(&mut self, _controller: ControllerBuilder<C>) {
        todo!()
    }

    /// Start the manager, blocking forever.
    pub async fn start(&mut self) {}
}
