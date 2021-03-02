use std::collections::HashMap;

use tokio::io::{AsyncRead, AsyncSeek};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::container::{
    ContainerKey, ContainerMapByName, Handle as ContainerHandle, HandleMap as ContainerHandleMap,
};
use crate::handle::StopHandler;
use crate::log::{HandleFactory, Sender};
use crate::pod::Pod;
use crate::provider::ProviderError;
use crate::volume::Ref;

/// Handle is the top level handle into managing a pod. It manages updating
/// statuses for the containers in the pod and can be used to stop the pod and
/// access logs
pub struct Handle<H, F> {
    container_handles: RwLock<ContainerHandleMap<H, F>>,
    pod: Pod,
    // Storage for the volume references so they don't get dropped until the runtime handle is
    // dropped
    // TODO: remove this; this is now part of the ModuleRunContext
    _volumes: HashMap<String, Ref>,
}

impl<H, F> std::fmt::Debug for Handle<H, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("pod", &self.pod.name())
            .finish()
    }
}

impl<H: StopHandler, F> Handle<H, F> {
    /// Creates a new pod handle that manages the given map of container names to
    /// [`ContainerHandle`]s. The given pod and client are used to maintain a reference to the
    /// kubernetes object and to be able to update the status of that object. The optional volumes
    /// parameter allows a caller to pass a map of volumes to keep reference to (so that they will
    /// be dropped along with the pod)
    pub fn new(
        container_handles: ContainerHandleMap<H, F>,
        pod: Pod,
        volumes: Option<HashMap<String, Ref>>,
    ) -> Self {
        Self {
            container_handles: RwLock::new(container_handles),
            pod,
            _volumes: volumes.unwrap_or_default(),
        }
    }

    /// Insert container `Handle` by `ContainerKey`.
    pub async fn insert_container_handle(&self, key: ContainerKey, value: ContainerHandle<H, F>) {
        let mut map = self.container_handles.write().await;
        map.insert(key, value);
    }

    /// Streams output from the specified container into the given sender.
    /// Optionally tails the output and/or continues to watch the file and stream changes.
    pub async fn output<R>(&self, container_name: &str, sender: Sender) -> anyhow::Result<()>
    where
        R: AsyncRead + AsyncSeek + Unpin + Send + 'static,
        F: HandleFactory<R>,
    {
        let mut handles = self.container_handles.write().await;
        let handle = handles
            .get_mut_by_name(container_name.to_owned())
            .ok_or_else(|| ProviderError::ContainerNotFound {
                pod_name: self.pod.name().to_owned(),
                container_name: container_name.to_owned(),
            })?;
        handle.output(sender).await
    }

    /// Signal the pod and all its running containers to stop and wait for them
    /// to complete.
    pub async fn stop(&self) -> anyhow::Result<()> {
        {
            let mut handles = self.container_handles.write().await;
            for (key, handle) in handles.iter_mut() {
                info!("Stopping container: {}", key);
                match handle.stop().await {
                    Ok(_) => debug!("Successfully stopped container {}", key),
                    // NOTE: I am not sure what recovery or retry steps should be
                    // done here, but we should definitely continue and try to stop
                    // the other containers
                    Err(e) => error!("Error while trying to stop pod {}: {:?}", key, e),
                }
            }
        }
        Ok(())
    }

    /// Wait for all containers in the pod to complete
    pub async fn wait(&mut self) -> anyhow::Result<()> {
        let mut handles = self.container_handles.write().await;
        for (_, handle) in handles.iter_mut() {
            handle.wait().await?;
        }
        Ok(())
    }
}

/// Generates a unique human readable key for storing a handle to a pod in a
/// hash. This is a convenience wrapper around [pod_key].
#[deprecated(
    since = "0.6.0",
    note = "Please use the new kubelet::pod::PodKey type. This function will be removed in 0.7"
)]
pub fn key_from_pod(pod: &Pod) -> String {
    #[allow(deprecated)]
    pod_key(pod.namespace(), pod.name())
}

/// Generates a unique human readable key for storing a handle to a pod if you
/// already have the namespace and pod name.
#[deprecated(
    since = "0.6.0",
    note = "Please use the new kubelet::pod::PodKey type. This function will be removed in 0.7"
)]
pub fn pod_key<N: AsRef<str>, T: AsRef<str>>(namespace: N, pod_name: T) -> String {
    format!("{}:{}", namespace.as_ref(), pod_name.as_ref())
}
