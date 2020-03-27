//! Container statuses
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::ContainerStatus as KubeContainerStatus;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateRunning, ContainerStateTerminated, ContainerStateWaiting,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

use std::collections::HashMap;

/// Describe the status of a workload.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Allows a provider to set a custom message, otherwise, kubelet will infer
    /// a message from the container statuses
    pub message: Option<String>,
    /// The statuses of containers keyed off their names
    pub container_statuses: HashMap<String, ContainerStatus>,
}

/// ContainerStatus is a simplified version of the Kubernetes container status
/// for use in providers. It allows for simple creation of the current status of
/// a "container" (a running wasm process) without worrying about a bunch of
/// Options. Use the [ContainerStatus::to_kubernetes] method for converting it
/// to a Kubernetes API container status
#[derive(Clone, Debug)]
pub enum ContainerStatus {
    /// The container is in a waiting state
    Waiting {
        /// The timestamp of when this status was reported
        timestamp: DateTime<Utc>,
        /// A human readable string describing the why it is in a waiting status
        message: String,
    },
    /// The container is running
    Running {
        /// The timestamp of when this status was reported
        timestamp: DateTime<Utc>,
    },
    /// The container is terminated
    Terminated {
        /// The timestamp of when this status was reported
        timestamp: DateTime<Utc>,
        /// A human readable string describing the why it is in a terminating status
        message: String,
        /// Should be set to true if the process exited with an error
        failed: bool,
    },
}

impl ContainerStatus {
    /// Convert the container status to a Kubernetes API compatible type
    pub fn to_kubernetes(&self, container_name: String) -> KubeContainerStatus {
        let mut state = ContainerState::default();
        match self {
            Self::Waiting { message, .. } => {
                state.waiting.replace(ContainerStateWaiting {
                    message: Some(message.clone()),
                    ..Default::default()
                });
            }
            Self::Running { timestamp } => {
                state.running.replace(ContainerStateRunning {
                    started_at: Some(Time(*timestamp)),
                });
            }
            Self::Terminated {
                timestamp,
                message,
                failed,
            } => {
                state.terminated.replace(ContainerStateTerminated {
                    finished_at: Some(Time(*timestamp)),
                    message: Some(message.clone()),
                    exit_code: *failed as i32,
                    ..Default::default()
                });
            }
        };
        let ready = state.running.is_some();
        KubeContainerStatus {
            state: Some(state),
            name: container_name,
            // Right now we don't have a way to probe, so just set to ready if
            // in a running state
            ready,
            // This is always true if startupProbe is not defined. When we
            // handle probes, this should be updated accordingly
            started: Some(true),
            // The rest of the items in status (see docs here:
            // https://kubernetes.io/docs/reference/generated/kubernetes-api/v1.17/#containerstatus-v1-core)
            // either don't matter for us or we have not implemented the
            // functionality yet
            ..Default::default()
        }
    }
}

/// Describe the lifecycle phase of a workload.
///
/// This is specified by Kubernetes itself.
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) enum Phase {
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
