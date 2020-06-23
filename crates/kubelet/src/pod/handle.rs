use std::collections::HashMap;

use log::{debug, error, info};
use tokio::io::{AsyncRead, AsyncSeek};
use tokio::stream::{StreamExt, StreamMap};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::container::Handle as ContainerHandle;
use crate::handle::StopHandler;
use crate::log::{HandleFactory, Sender};
use crate::pod::Pod;
use crate::pod::Status;
use crate::provider::ProviderError;
use crate::volume::Ref;

/// Handle is the top level handle into managing a pod. It manages updating
/// statuses for the containers in the pod and can be used to stop the pod and
/// access logs
pub struct Handle<H, F> {
    container_handles: RwLock<HashMap<String, ContainerHandle<H, F>>>,
    status_handle: JoinHandle<()>,
    pod: Pod,
    // Storage for the volume references so they don't get dropped until the runtime handle is
    // dropped
    _volumes: HashMap<String, Ref>,
}

impl<H: StopHandler, F> Handle<H, F> {
    /// Creates a new pod handle that manages the given map of container names to
    /// [`ContainerHandle`]s. The given pod and client are used to maintain a reference to the
    /// kubernetes object and to be able to update the status of that object. The optional volumes
    /// parameter allows a caller to pass a map of volumes to keep reference to (so that they will
    /// be dropped along with the pod)
    pub fn new(
        container_handles: HashMap<String, ContainerHandle<H, F>>,
        pod: Pod,
        client: kube::Client,
        volumes: Option<HashMap<String, Ref>>,
    ) -> anyhow::Result<Self> {
        let mut channel_map = StreamMap::with_capacity(container_handles.len());
        for (name, handle) in container_handles.iter() {
            channel_map.insert(name.clone(), handle.status());
        }
        // TODO: This does not allow for restarting single containers because we
        // move the stream map and lose the ability to insert a new channel for
        // the restarted runtime. It may involve sending things to the task with
        // a channel
        let cloned_pod = pod.clone();
        let status_handle = tokio::task::spawn(async move {
            loop {
                let (name, status) = match channel_map.next().await {
                    Some(s) => s,
                    // None means everything is closed, so go ahead and exit
                    None => return,
                };
                debug!("Got status update from container {}: {:#?}", name, status);
                let mut container_statuses = HashMap::new();
                container_statuses.insert(name, status);
                let status = Status {
                    message: None,
                    container_statuses,
                };
                cloned_pod.patch_status(client.clone(), status).await;
            }
        });
        Ok(Self {
            container_handles: RwLock::new(container_handles),
            status_handle,
            pod,
            _volumes: volumes.unwrap_or_default(),
        })
    }

    /// Streams output from the specified container into the given sender.
    /// Optionally tails the output and/or continues to watch the file and stream changes.
    pub async fn output<R>(&mut self, container_name: &str, sender: Sender) -> anyhow::Result<()>
    where
        R: AsyncRead + AsyncSeek + Unpin + Send + 'static,
        F: HandleFactory<R>,
    {
        let mut handles = self.container_handles.write().await;
        let handle =
            handles
                .get_mut(container_name)
                .ok_or_else(|| ProviderError::ContainerNotFound {
                    pod_name: self.pod.name().to_owned(),
                    container_name: container_name.to_owned(),
                })?;
        handle.output(sender).await
    }

    /// Signal the pod and all its running containers to stop and wait for them
    /// to complete. As of right now, there is not a way to do this in wasmtime,
    /// so this does nothing
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        {
            let mut handles = self.container_handles.write().await;
            for (name, handle) in handles.iter_mut() {
                info!("Stopping container: {}", name);
                match handle.stop().await {
                    Ok(_) => debug!("Successfully stopped container {}", name),
                    // NOTE: I am not sure what recovery or retry steps should be
                    // done here, but we should definitely continue and try to stop
                    // the other containers
                    Err(e) => error!("Error while trying to stop pod {}: {:?}", name, e),
                }
            }
        }
        Ok(())
    }

    /// Wait for all containers in the pod to complete
    pub async fn wait(&mut self) -> anyhow::Result<()> {
        let mut handles = self.container_handles.write().await;
        for (name, handle) in handles.iter_mut() {
            debug!("Waiting for container {} to terminate", name);
            handle.wait().await?;
        }
        (&mut self.status_handle).await?;
        Ok(())
    }
}

/// Generates a unique human readable key for storing a handle to a pod in a
/// hash. This is a convenience wrapper around [pod_key].
pub fn key_from_pod(pod: &Pod) -> String {
    pod_key(pod.namespace(), pod.name())
}

/// Generates a unique human readable key for storing a handle to a pod if you
/// already have the namespace and pod name.
pub fn pod_key<N: AsRef<str>, T: AsRef<str>>(namespace: N, pod_name: T) -> String {
    format!("{}:{}", namespace.as_ref(), pod_name.as_ref())
}
