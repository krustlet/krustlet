use kube::{
    api::{DynamicObject, GroupVersionKind, ListParams},
    Resource,
};
use kube_runtime::watcher::Event;

/// Captures configuration needed to configure a watcher.
#[derive(Clone, Debug)]
pub struct Watch {
    /// The (group, version, kind) tuple of the resource to be watched.
    pub gvk: GroupVersionKind,
    /// Optionally restrict watching to namespace.
    pub namespace: Option<String>,
    /// Restrict to objects matching list params (default watches everything).
    pub list_params: ListParams,
}

impl Watch {
    pub fn new<
        R: Resource<DynamicType = ()> + serde::de::DeserializeOwned + Clone + Send + 'static,
    >(
        namespace: Option<String>,
        list_params: ListParams,
    ) -> Self {
        let gvk = GroupVersionKind::gvk(&R::group(&()), &R::version(&()), &R::kind(&())).unwrap();
        Watch {
            gvk,
            namespace,
            list_params,
        }
    }

    pub fn handle(
        self,
        buffer: usize,
    ) -> (
        WatchHandle,
        tokio::sync::mpsc::Receiver<Event<DynamicObject>>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(buffer);
        let handle = WatchHandle { watch: self, tx };
        (handle, rx)
    }
}

#[derive(Clone)]
pub struct WatchHandle {
    pub watch: Watch,
    pub tx: tokio::sync::mpsc::Sender<Event<DynamicObject>>,
}
