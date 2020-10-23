//! Container statuses

use super::Pod;
use crate::container::{make_initial_container_status, ContainerMap, Status as ContainerStatus};
use k8s_openapi::api::core::v1::ContainerStatus as KubeContainerStatus;

/// Initialize Pod status.
/// This initializes Pod status to include containers in the correct order as expected by
/// `patch_container_status`.
pub fn make_registered_status(pod: &Pod) -> anyhow::Result<serde_json::Value> {
    let init_container_statuses: Vec<KubeContainerStatus> = pod
        .init_containers()
        .iter()
        .map(make_initial_container_status)
        .collect();
    let container_statuses: Vec<KubeContainerStatus> = pod
        .containers()
        .iter()
        .map(make_initial_container_status)
        .collect();
    make_status_with_containers(
        Phase::Pending,
        "Registered",
        container_statuses,
        init_container_statuses,
    )
}

/// Create basic Pod status patch.
pub fn make_status(phase: Phase, reason: &str) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::json!(
       {
           "metadata": {
               "resourceVersion": "",
           },
           "status": {
               "phase": phase,
               "reason": reason,
           }
       }
    ))
}

/// Create basic Pod status patch.
pub fn make_status_with_containers(
    phase: Phase,
    reason: &str,
    container_statuses: Vec<KubeContainerStatus>,
    init_container_statuses: Vec<KubeContainerStatus>,
) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::json!(
       {
           "metadata": {
               "resourceVersion": "",
           },
           "status": {
               "phase": phase,
               "reason": reason,
               "containerStatuses": container_statuses,
               "initContainerStatuses": init_container_statuses,
           }
       }
    ))
}

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
    /// The pod is being created.
    Pending,
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
