//! Container statuses

use k8s_openapi::api::core::v1::Pod;
use kube::{api::PatchParams, Api};

use crate::container::{ContainerMap, Status as ContainerStatus};

/// Describe the status of a workload.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Allows a provider to set a custom message, otherwise, kubelet will infer
    /// a message from the container statuses
    pub message: StatusMessage,
    /// The statuses of containers keyed off their names
    pub container_statuses: ContainerMap<ContainerStatus>,
}

#[derive(Clone, Debug)]
/// The message to be set in a pod status update.
pub enum StatusMessage {
    /// Do not change the existing status message.
    LeaveUnchanged,
    /// Remove any existing status message.
    Clear,
    /// Set the status message to the given value.
    Message(String),
}

impl Default for StatusMessage {
    fn default() -> Self {
        Self::LeaveUnchanged
    }
}

/// Describe the lifecycle phase of a workload.
///
/// This is specified by Kubernetes itself.
#[derive(Clone, Debug, serde::Serialize)]
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

impl Default for Phase {
    fn default() -> Self {
        Self::Unknown
    }
}

/// A helper for updating pod status. The given data should be a pod status object and be
/// serializable by serde
pub async fn update_status<T: serde::Serialize>(
    client: kube::Client,
    ns: &str,
    pod_name: &str,
    data: &T,
) -> anyhow::Result<()> {
    let data = serde_json::to_vec(data)?;
    let pod_client: Api<Pod> = Api::namespaced(client, ns);
    if let Err(e) = pod_client
        .patch_status(pod_name, &PatchParams::default(), data)
        .await
    {
        return Err(e.into());
    }
    Ok(())
}
