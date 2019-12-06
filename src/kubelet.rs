/// This library contans the Kubelet shell. Use this to create a new Kubelet
/// with a specific handler. (The handler included here is the WASM handler.)
use crate::pod::KubePod;
use kube::client::APIClient;

#[derive(Fail, Debug)]
#[fail(display = "Operation not supported")]
pub struct NotImplementedError;

/// Describe the lifecycle phase of a workload.
pub enum Phase {
    /// The workload is currently executing.
    Running,
    /// The workload has exited with an error.
    Failed,
    /// The workload has exited without error.
    Succeeded,
    /// The lifecycle phase of the workload cannot be determined.
    Unknown,
}

/// Describe the status of a workload.
///
/// Phase captures the lifecycle aspect of the workload, while
/// the message provides a human-readable description of the
/// state of the workload.
pub struct Status {
    pub phase: Phase,
    pub message: Option<String>,
}

// Kubelet provides the core Kubelet capability.
pub struct Kubelet<P: Provider> {
    provider: P,
}

impl<T: Provider> Kubelet<T> {
    pub fn handle_event(&self) -> Result<(), failure::Error> {
        Ok(())
    }
}

// Provider implements the back-end for the Kubelet.
pub trait Provider {
    /// Given a Pod definition, this function determines whether or not the workload is schedulable.
    fn can_schedule(pod: &KubePod) -> bool;
    /// Given a Pod definition, execute the workload.
    fn add(&self, pod: KubePod, client: APIClient) -> Result<(), failure::Error>;
    /// Given an updated Pod definition, update the given workload.
    ///
    /// Pods that are sent to this function have already met certain criteria for modification.
    /// For example, updates to the `status` of a Pod will not be sent into this function.
    fn modify(&self, pod: KubePod, client: APIClient) -> Result<(), failure::Error>;
    /// Given a pod, determine the status of the underlying workload.
    ///
    /// This information is used to update Kubernetes about whether this workload is running,
    /// has already finished running, or has failed.
    fn status(&self, pod: KubePod, client: APIClient) -> Result<Status, failure::Error>;
    /// Given the definition of a deleted Pod, remove the workload from the runtime.
    ///
    /// This does not need to actually delete the Pod definition -- just destroy the
    /// associated workload. The default implementation simply returns Ok.
    fn delete(&self, _pod: KubePod, _client: APIClient) -> Result<(), failure::Error> {
        Ok(())
    }
    /// Given a Pod, get back the logs for the associated workload.
    ///
    /// The default implementation of this returns a message that this feature is
    /// not available. Override this only when there is an implementation available.
    fn logs(&self, _pod: KubePod, _client: APIClient) -> Result<Vec<String>, failure::Error> {
        Err(NotImplementedError {}.into())
    }
    /// Execute a given command on a workload and then return the result.
    ///
    /// The default implementation of this returns a message that this feature is
    /// not available. Override this only when there is an implementation.
    fn exec(&self, 
        _pod: KubePod,
        _client: APIClient,
        _command: String,
    ) -> Result<Vec<String>, failure::Error> {
        Err(NotImplementedError {}.into())
    }
}
