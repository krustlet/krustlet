/// A [`StopHandler`] is used to handle stopping running processes.
#[async_trait::async_trait]
pub trait StopHandler {
    /// Calling stop should sends a signal for anything running under the implementor to stop.
    ///
    /// This is considered an ungraceful stop, and the caller should not wait for the
    /// underlying handle to complete. Instead they should call wait() to wait for anything running
    /// to stop.
    async fn stop(&mut self) -> anyhow::Result<()>;
    /// Wait for the implementor to stop anything it considers in the running state.
    async fn wait(&mut self) -> anyhow::Result<()>;
}
