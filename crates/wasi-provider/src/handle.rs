use std::io::SeekFrom;

use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::watch::Receiver;
use tokio::task::JoinHandle;

use kubelet::ContainerStatus;

/// Represents a handle to a running WASI instance. Right now, this is
/// experimental and just for use with the [crate::WasiProvider]. If we like
/// this pattern, we will expose it as part of the kubelet crate
pub struct RuntimeHandle<R: AsyncReadExt + AsyncSeekExt + Unpin> {
    output: BufReader<R>,
    handle: JoinHandle<anyhow::Result<()>>,
    status_channel: Receiver<ContainerStatus>,
}

impl<R: AsyncReadExt + AsyncSeekExt + Unpin> RuntimeHandle<R> {
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
    pub async fn output(&mut self, buf: &mut Vec<u8>) -> anyhow::Result<usize> {
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
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        // TODO: Send an actual stop signal once there is support in wasmtime
        self.wait().await?;
        unimplemented!("There is currently no way to stop a running wasmtime instance")
    }

    /// Returns the current status of the process
    pub async fn status(&self) -> anyhow::Result<ContainerStatus> {
        // NOTE: For those who modify this in the future, borrow must be as
        // short lived as possible as it can block the send half. We do not use
        // the recv method because it uses the value each time and then waits
        // for a new value on the next call, whereas we want to return the last
        // sent value until updated
        Ok(self.status_channel.borrow().clone())
    }

    // For now this is private (for use in testing and in stop). If we find a
    // need to expose it, we can do that later
    pub(crate) async fn wait(&mut self) -> anyhow::Result<()> {
        (&mut self.handle).await.unwrap()
    }
}
