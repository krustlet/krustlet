use crate::runtime::PrettyEvent;
use kube::{
    api::{DynamicObject, GroupVersionKind, ListParams},
    Resource,
};
use kube_runtime::watcher::Event;
use tracing::{info, warn};

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
    ) -> (
        WatchHandle,
        tokio::sync::mpsc::Receiver<Event<DynamicObject>>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let handle = WatchHandle { watch: self, tx };
        (handle, rx)
    }
}

#[derive(Clone)]
pub struct WatchHandle {
    pub watch: Watch,
    pub tx: tokio::sync::mpsc::Sender<Event<DynamicObject>>,
}

pub async fn launch_watcher(client: kube::Client, handle: WatchHandle) {
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
                info!(
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
