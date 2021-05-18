use super::watch::{Watch, WatchHandle};
#[cfg(feature = "admission-webhook")]
use crate::admission::WebhookFn;
use crate::Operator;
use kube::api::ListParams;
use kube::Resource;

/// Builder pattern for registering a controller or operator.
pub struct ControllerBuilder<C: Operator> {
    /// The controller or operator singleton.
    pub(crate) controller: C,
    ///  List of watch configurations for objects that will simply be cached
    ///  locally.
    pub(crate) watches: Vec<Watch>,
    /// List of watch configurations for objects that will trigger
    /// notifications (based on OwnerReferences).
    pub(crate) owns: Vec<Watch>,
    /// Restrict our controller to act on a specific namespace.
    namespace: Option<String>,
    /// Restrict our controller to act on objects that match specific list
    /// params.
    list_params: ListParams,
    /// The buffer length for Tokio channels used to communicate between
    /// watcher tasks and runtime tasks.
    buffer: usize,
}

/// Trait alias for types which can be watched.
pub trait Watchable:
    Resource<DynamicType = ()> + serde::de::DeserializeOwned + Clone + Send + 'static
{
}

impl<T> Watchable for T where
    T: Resource<DynamicType = ()> + serde::de::DeserializeOwned + Clone + Send + 'static
{
}

impl<O: Operator> ControllerBuilder<O> {
    /// Create builder from operator singleton.
    pub fn new(operator: O) -> Self {
        ControllerBuilder {
            controller: operator,
            watches: vec![],
            owns: vec![],
            namespace: None,
            list_params: Default::default(),
            buffer: 32,
        }
    }

    /// Change the length of buffer used for internal communication channels.
    pub fn with_buffer(mut self, buffer: usize) -> Self {
        self.buffer = buffer;
        self
    }

    pub(crate) fn buffer(&self) -> usize {
        self.buffer
    }

    /// Create watcher definition for the configured managed resource.
    pub(crate) fn manages(&self) -> Watch {
        Watch::new::<O::Manifest>(self.namespace.clone(), self.list_params.clone())
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
        R: Watchable,
    {
        self.watches.push(Watch::new::<R>(None, Default::default()));
        self
    }

    /// Watch objects of given kind R. Cluster scoped, but limited to objects
    /// matching supplied list params.
    pub fn watches_with_params<R>(mut self, list_params: ListParams) -> Self
    where
        R: Watchable,
    {
        self.watches.push(Watch::new::<R>(None, list_params));
        self
    }

    /// Watch all objects of given kind R in supplied namespace, with no list
    /// param restrictions.
    pub fn watches_namespaced<R>(mut self, namespace: &str) -> Self
    where
        R: Watchable,
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
        R: Watchable,
    {
        self.watches
            .push(Watch::new::<R>(Some(namespace.to_string()), list_params));
        self
    }

    /// Watch and subscribe to notifications based on OwnerReferences all
    /// objects of kind R. Cluster scoped and no list param restrictions.
    pub fn owns<R>(mut self) -> Self
    where
        R: Watchable,
    {
        self.owns.push(Watch::new::<R>(None, Default::default()));
        self
    }

    /// Watch and subscribe to notifications based on OwnerReferences
    /// objects of kind R. Cluster scoped, but limited to objects matching
    /// supplied list params.
    pub fn owns_with_params<R>(mut self, list_params: ListParams) -> Self
    where
        R: Watchable,
    {
        self.owns.push(Watch::new::<R>(None, list_params));
        self
    }

    /// Watch and subscribe to notifications based on OwnerReferences
    /// objects of kind R in supplied namespace, with no list param
    /// restrictions.
    pub fn owns_namespaced<R>(mut self, namespace: &str) -> Self
    where
        R: Watchable,
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
        R: Watchable,
    {
        self.owns
            .push(Watch::new::<R>(Some(namespace.to_string()), list_params));
        self
    }

    /// Registers a validating webhook at the path "/$GROUP/$VERSION/$KIND".
    /// Multiple webhooks can be registered, but must be at different paths.
    #[cfg(feature = "admission-webhook")]
    pub(crate) fn validates(self, _f: &WebhookFn<O>) -> Self {
        todo!()
    }

    /// Registers a validating webhook at the supplied path.
    #[cfg(feature = "admission-webhook")]
    pub(crate) fn validates_at_path(self, _path: &str, _f: &WebhookFn<O>) -> Self {
        todo!()
    }

    /// Registers a mutating webhook at the path "/$GROUP/$VERSION/$KIND".
    /// Multiple webhooks can be registered, but must be at different paths.
    #[cfg(feature = "admission-webhook")]
    pub(crate) fn mutates(self, _f: &WebhookFn<O>) -> Self {
        todo!()
    }

    /// Registers a mutating webhook at the supplied path.
    #[cfg(feature = "admission-webhook")]
    pub(crate) fn mutates_at_path(self, _path: &str, _f: &WebhookFn<O>) -> Self {
        todo!()
    }
}

#[derive(Clone)]
pub struct Controller {
    pub manages: WatchHandle,
    pub owns: Vec<WatchHandle>,
    pub watches: Vec<WatchHandle>,
}
