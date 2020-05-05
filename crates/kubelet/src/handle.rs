//! A convenience handle type for providers
//!
//! A collection of handle types for use in providers. These are entirely
//! optional, but abstract away much of the logic around managing logging,
//! status updates, and stopping pods

use anyhow::bail;
use std::collections::HashMap;
use std::io::SeekFrom;

use log::{debug, error, info};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncSeek, AsyncSeekExt};
use tokio::stream::{StreamExt, StreamMap};
use tokio::sync::watch::Receiver;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::provider::ProviderError;
use crate::status::{ContainerStatus, Status};
use crate::volumes::VolumeRef;
use crate::Pod;

/// Any provider wanting to use the [`RuntimeHandle`] and
/// [`PodHandle`] will need to have some sort of "stopper" that implement
/// this Trait. Because the logic for stopping a running "container" can vary
/// from provider to provider, this allows for flexibility in implementing how
/// to stop each runtime
#[async_trait::async_trait]
pub trait Stop {
    /// Should send a signal for the running process to stop. It should not wait
    /// for the process to complete
    async fn stop(&mut self) -> anyhow::Result<()>;
    /// Wait for the running process to complete.
    async fn wait(&mut self) -> anyhow::Result<()>;
}

/// Trait to describe necessary behavior for creating multiple log readers.
/// TODO: Both providers make a handle containing a tempfile. If this is a common pattern,
/// it might make sense to provide that implementation here. This would add `tempfile` as a
/// dependency of `kubelet`.
pub trait LogHandle<R>: Sync + Send {
    /// Create new log reader.
    fn output(&self) -> R;
}

/// Future that streams logs from provided `AsyncRead` to provided `hyper::body::Sender`.
async fn stream_logs<R: AsyncRead + std::marker::Unpin>(
    output: R,
    mut sender: hyper::body::Sender,
    tail: Option<usize>,
    follow: bool,
) -> anyhow::Result<()> {
    let buf = tokio::io::BufReader::new(output);
    let mut lines = buf.lines();

    if let Some(n) = tail {
        // Stream last n lines.
        // TODO: this uses a lot of memory for large n and scans the entire file.
        let mut line_buf = std::collections::VecDeque::with_capacity(n);

        while let Some(line) = match lines.next_line().await {
            Ok(line) => line,
            Err(e) => {
                let err = format!("Error reading from log: {:?}", e);
                error!("{}", &err);
                let b = hyper::body::Bytes::copy_from_slice(&err.as_bytes());
                sender.send_data(b).await?;
                bail!(e);
            }
        } {
            if line_buf.len() == n {
                line_buf.pop_front();
            }
            line_buf.push_back(line);
        }

        for mut line in line_buf {
            line.push('\n');
            let b = hyper::body::Bytes::copy_from_slice(&line.as_bytes());
            match sender.send_data(b).await {
                Ok(_) => (),
                Err(e) => {
                    if e.is_closed() {
                        debug!("channel closed.");
                        return Ok(());
                    } else {
                        error!("channel error: {}", e);
                        bail!(e);
                    }
                }
            }
        }
    } else {
        // Stream entire file.
        while let Some(mut line) = match lines.next_line().await {
            Ok(line) => line,
            Err(e) => {
                let err = format!("Error reading from log: {:?}", e);
                error!("{}", &err);
                let b = hyper::body::Bytes::copy_from_slice(&err.as_bytes());
                sender.send_data(b).await?;
                bail!(e);
            }
        } {
            line.push('\n');
            let b = hyper::body::Bytes::copy_from_slice(&line.as_bytes());
            match sender.send_data(b).await {
                Ok(_) => (),
                Err(e) => {
                    if e.is_closed() {
                        debug!("channel closed.");
                        return Ok(());
                    } else {
                        error!("channel error: {}", e);
                        bail!(e);
                    }
                }
            }
        }
    }

    if follow {
        // Optionally watch file for changes.
        loop {
            while let Some(mut line) = match lines.next_line().await {
                Ok(line) => line,
                Err(e) => {
                    let err = format!("Error reading from log: {:?}", e);
                    error!("{}", &err);
                    let b = hyper::body::Bytes::copy_from_slice(&err.as_bytes());
                    sender.send_data(b).await?;
                    bail!(e);
                }
            } {
                line.push('\n');
                let b = hyper::body::Bytes::copy_from_slice(&line.as_bytes());
                match sender.send_data(b).await {
                    Ok(_) => (),
                    Err(e) => {
                        if e.is_closed() {
                            debug!("channel closed.");
                            return Ok(());
                        } else {
                            error!("channel error: {}", e);
                            bail!(e);
                        }
                    }
                }
            }
            tokio::time::delay_for(std::time::Duration::from_millis(500)).await;
        }
    }
    Ok(())
}

/// Represents a handle to a running "container" (whatever that might be). This
/// can be used on its own, however, it is generally better to use it as a part
/// of a [`PodHandle`], which manages a group of containers in a Kubernetes
/// Pod
pub struct RuntimeHandle<S, H> {
    stopper: S,
    handle: H,
    status_channel: Receiver<ContainerStatus>,
}

impl<S: Stop, H> RuntimeHandle<S, H> {
    /// Create a new handle with the given stopper for stopping the runtime,
    /// a reader for log output and status channel.
    ///
    /// The status channel is a [Tokio watch `Receiver`][Receiver]. The sender part
    /// of the channel should be given to the running process and the receiver half
    /// passed to this constructor to be used for reporting current status
    pub fn new(stopper: S, handle: H, status_channel: Receiver<ContainerStatus>) -> Self {
        Self {
            stopper,
            handle,
            status_channel,
        }
    }

    /// Signal the running instance to stop. Use [`RuntimeHandle::wait`] to wait for the process to
    /// exit. This uses the underlying [`Stop`] implementation passed to the constructor
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.stopper.stop().await
    }

    /// Write all of the output from the running process into the given buffer.
    /// Returns the number of bytes written to the buffer
    pub(crate) async fn output<R>(
        &mut self,
        sender: hyper::body::Sender,
        tail: Option<usize>,
        follow: bool,
    ) -> anyhow::Result<()>
    where
        R: AsyncRead + AsyncSeek + Unpin + Send + 'static,
        H: LogHandle<R>,
    {
        let mut output = self.handle.output();
        output.seek(SeekFrom::Start(0)).await?;
        tokio::spawn(stream_logs(output, sender, tail, follow));
        Ok(())
    }

    /// Returns a clone of the status_channel for use in reporting the status to
    /// another process
    pub(crate) fn status(&self) -> Receiver<ContainerStatus> {
        self.status_channel.clone()
    }

    /// Wait for the running process to complete. Generally speaking,
    /// [`RuntimeHandle::stop`] should be called first. This uses the underlying
    /// [`Stop`] implementation passed to the constructor
    pub(crate) async fn wait(&mut self) -> anyhow::Result<()> {
        self.stopper.wait().await
    }
}

/// PodHandle is the top level handle into managing a pod. It manages updating
/// statuses for the containers in the pod and can be used to stop the pod and
/// access logs
pub struct PodHandle<S, H> {
    container_handles: RwLock<HashMap<String, RuntimeHandle<S, H>>>,
    status_handle: JoinHandle<()>,
    pod: Pod,
    // Storage for the volume references so they don't get dropped until the runtime handle is
    // dropped
    _volumes: HashMap<String, VolumeRef>,
}

impl<S: Stop, H> PodHandle<S, H> {
    /// Creates a new pod handle that manages the given map of container names to
    /// [`RuntimeHandle`]s. The given pod and client are used to maintain a reference to the
    /// kubernetes object and to be able to update the status of that object. The optional volumes
    /// parameter allows a caller to pass a map of volumes to keep reference to (so that they will
    /// be dropped along with the pod)
    pub fn new(
        container_handles: HashMap<String, RuntimeHandle<S, H>>,
        pod: Pod,
        client: kube::Client,
        volumes: Option<HashMap<String, VolumeRef>>,
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

    /// Write all of the output from the specified container into the given
    /// buffer. Returns the number of bytes written to the buffer
    pub async fn output<R>(
        &mut self,
        container_name: &str,
        sender: hyper::body::Sender,
        tail: Option<usize>,
        follow: bool,
    ) -> anyhow::Result<()>
    where
        R: AsyncRead + AsyncSeek + Unpin + Send + 'static,
        H: LogHandle<R>,
    {
        let mut handles = self.container_handles.write().await;
        let handle =
            handles
                .get_mut(container_name)
                .ok_or_else(|| ProviderError::ContainerNotFound {
                    pod_name: self.pod.name().to_owned(),
                    container_name: container_name.to_owned(),
                })?;
        handle.output(sender, tail, follow).await
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
