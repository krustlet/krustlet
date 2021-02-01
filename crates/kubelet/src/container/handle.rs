use std::io::SeekFrom;

use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt};

use crate::container::ContainerMap;
use crate::handle::StopHandler;
use crate::log::{stream, HandleFactory, Sender};

/// Represents a handle to a running "container" (whatever that might be). This
/// can be used on its own, however, it is generally better to use it as a part
/// of a [`crate::pod::Handle`], which manages a group of containers in a Kubernetes
/// Pod
pub struct Handle<H, F> {
    handle: H,
    handle_factory: F,
}

impl<H, F> std::fmt::Debug for Handle<H, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerHandle").finish()
    }
}

impl<H: StopHandler, F> Handle<H, F> {
    /// Create a new runtime with the given handle for stopping the runtime,
    /// a reader for log output, and a status channel.
    pub fn new(handle: H, handle_factory: F) -> Self {
        Self {
            handle,
            handle_factory,
        }
    }

    /// Signal the running instance to stop. Use [`Handle::wait`] to wait for the process to
    /// exit. This uses the underlying [`StopHandler`] implementation passed to the constructor
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.handle.stop().await
    }

    /// Streams output from the running process into the given sender.
    /// Optionally tails the output and/or continues to watch the file and stream changes.
    pub(crate) async fn output<R>(&mut self, sender: Sender) -> anyhow::Result<()>
    where
        R: AsyncRead + AsyncSeek + Unpin + Send + 'static,
        F: HandleFactory<R>,
    {
        let mut handle = self.handle_factory.new_handle();
        handle.seek(SeekFrom::Start(0)).await?;
        tokio::spawn(stream(handle, sender));
        Ok(())
    }

    /// Wait for the running process to complete. Generally speaking,
    /// [`Handle::stop`] should be called first. This uses the underlying
    /// [`StopHandler`] implementation passed to the constructor
    pub async fn wait(&mut self) -> anyhow::Result<()> {
        self.handle.wait().await
    }
}

/// A map from containers to container handles.
pub type HandleMap<H, F> = ContainerMap<Handle<H, F>>;
