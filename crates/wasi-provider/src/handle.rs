use std::collections::HashMap;
use std::io::SeekFrom;

use kube::client::APIClient;
use log::{debug, error, info};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, BufReader};
use tokio::stream::{StreamExt, StreamMap};
use tokio::sync::watch::Receiver;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use kubelet::pod::Pod;
use kubelet::{ContainerStatus, ProviderError, Status};

/// Represents a handle to a running WASI instance. Right now, this is
/// experimental and just for use with the [crate::WasiProvider]. If we like
/// this pattern, we will expose it as part of the kubelet crate
pub struct RuntimeHandle<R: AsyncRead + AsyncSeek + Unpin> {
    output: BufReader<R>,
    handle: JoinHandle<anyhow::Result<()>>,
    status_channel: Receiver<ContainerStatus>,
}

impl<R: AsyncRead + AsyncSeek + Unpin> RuntimeHandle<R> {
    /// Create a new handle with the given reader for log output and a handle to
    /// the running tokio task. The sender part of the channel should be given
    /// to the running process and the receiver half passed to this constructor
    /// to be used for reporting current status
    pub fn new(
        output: R,
        handle: JoinHandle<anyhow::Result<()>>,
        status_channel: Receiver<ContainerStatus>,
    ) -> Self {
        RuntimeHandle {
            output: BufReader::new(output),
            handle,
            status_channel,
        }
    }

    /// Write all of the output from the running process into the given buffer.
    /// Returns the number of bytes written to the buffer
    pub(crate) async fn output(&mut self, buf: &mut Vec<u8>) -> anyhow::Result<usize> {
        let bytes_written = self.output.read_to_end(buf).await?;
        // Reset the seek location for the next call to read from the file
        // NOTE: The Tokio BufReader does not implement seek, so we need to get
        // a mutable ref to the inner file and perform the seek
        self.output.get_mut().seek(SeekFrom::Start(0)).await?;
        Ok(bytes_written)
    }

    /// Signal the running instance to stop and wait for it to complete. As of
    /// right now, there is not a way to do this in wasmtime, so this does
    /// nothing
    pub(crate) async fn stop(&mut self) -> anyhow::Result<()> {
        // TODO: Send an actual stop signal once there is support in wasmtime
        unimplemented!("There is currently no way to stop a running wasmtime instance")
    }

    pub(crate) fn status(&self) -> Receiver<ContainerStatus> {
        self.status_channel.clone()
    }

    pub(crate) async fn wait(&mut self) -> anyhow::Result<()> {
        (&mut self.handle).await.unwrap()
    }
}

/// PodHandle is the top level handle into managing a pod. It manages updating
/// statuses for the containers in the pod and can be used to stop the pod and
/// access logs
pub struct PodHandle<R: AsyncRead + AsyncSeek + Unpin> {
    container_handles: RwLock<HashMap<String, RuntimeHandle<R>>>,
    // The channel for sending a stop signal to the status updater tasks
    status_handle: JoinHandle<()>,
    pod: Pod,
}

impl<R: AsyncRead + AsyncSeek + Unpin> PodHandle<R> {
    /// Creates a new pod handle that manages the given map of container names
    /// to [crate::RuntimeHandle]s. The given pod and client are used to
    /// maintain a reference to the kubernetes object and to be able to update
    /// the status of that object
    pub fn new(
        container_handles: HashMap<String, RuntimeHandle<R>>,
        pod: Pod,
        client: APIClient,
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
        Ok(PodHandle {
            container_handles: RwLock::new(container_handles),
            status_handle,
            pod,
        })
    }

    /// Write all of the output from the specified container into the given
    /// buffer. Returns the number of bytes written to the buffer
    pub async fn output(
        &mut self,
        container_name: &str,
        buf: &mut Vec<u8>,
    ) -> anyhow::Result<usize> {
        let mut handles = self.container_handles.write().await;
        let handle =
            handles
                .get_mut(container_name)
                .ok_or_else(|| ProviderError::ContainerNotFound {
                    pod_name: self.pod.name().to_owned(),
                    container_name: container_name.to_owned(),
                })?;
        handle.output(buf).await
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
        self.wait().await?;
        (&mut self.status_handle).await?;
        unimplemented!("There is currently no way to stop a running wasmtime instance")
    }

    /// Wait for all containers in the pod to complete
    pub async fn wait(&mut self) -> anyhow::Result<()> {
        let mut handles = self.container_handles.write().await;
        for (name, handle) in handles.iter_mut() {
            debug!("Waiting for container {} to terminate", name);
            handle.stop().await?;
        }
        Ok(())
    }
}
