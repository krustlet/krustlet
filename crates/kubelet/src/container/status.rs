use crate::container::{Container, ContainerKey};
use crate::pod::Pod;
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateRunning, ContainerStateTerminated, ContainerStateWaiting,
    ContainerStatus as KubeContainerStatus, Pod as KubePod,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use tracing::{debug, instrument, warn};

/// Status is a simplified version of the Kubernetes container status
/// for use in providers. It allows for simple creation of the current status of
/// a "container" (a running wasm process) without worrying about a bunch of
/// Options. Use the [Status::to_kubernetes] method for converting it
/// to a Kubernetes API container status
#[derive(Clone, Debug)]
pub enum Status {
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

impl Status {
    /// Create `Status::Waiting` from message.
    pub fn waiting(message: &str) -> Self {
        Status::Waiting {
            timestamp: Utc::now(),
            message: message.to_string(),
        }
    }

    /// Create `Status::Running`.
    pub fn running() -> Self {
        Status::Running {
            timestamp: Utc::now(),
        }
    }

    /// Create `Status::Terminated` from message and failed `bool`.
    pub fn terminated(message: &str, failed: bool) -> Self {
        Status::Terminated {
            timestamp: Utc::now(),
            message: message.to_string(),
            failed,
        }
    }

    /// Convert the container status to a Kubernetes API compatible type
    pub fn to_kubernetes(&self, container_name: &str) -> KubeContainerStatus {
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
            name: container_name.to_string(),
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

/// Patch a single container's status
#[instrument(level = "info", skip(client, pod, key, status), fields(pod_name = %pod.name(), namespace = %pod.namespace(), container_name = %key))]
pub async fn patch_container_status(
    client: &kube::Api<KubePod>,
    pod: &Pod,
    key: &ContainerKey,
    status: &Status,
) -> anyhow::Result<()> {
    match pod.find_container(key) {
        Some(container) => {
            let kube_status = status.to_kubernetes(container.name());

            let patches = match pod.container_status_index(key) {
                Some(idx) => {
                    let path_prefix = if key.is_init() {
                        format!("/status/initContainerStatuses/{}", idx)
                    } else {
                        format!("/status/containerStatuses/{}", idx)
                    };

                    vec![
                        json_patch::PatchOperation::Replace(json_patch::ReplaceOperation {
                            path: format!("{}/state", path_prefix),
                            value: serde_json::json!(kube_status.state.unwrap()),
                        }),
                        json_patch::PatchOperation::Replace(json_patch::ReplaceOperation {
                            path: format!("{}/ready", path_prefix),
                            value: serde_json::json!(kube_status.ready),
                        }),
                        json_patch::PatchOperation::Replace(json_patch::ReplaceOperation {
                            path: format!("{}/started", path_prefix),
                            value: serde_json::json!(true),
                        }),
                    ]
                }
                None => {
                    let path = if key.is_init() {
                        "/status/initContainerStatuses/-".to_string()
                    } else {
                        "/status/containerStatuses/-".to_string()
                    };

                    vec![json_patch::PatchOperation::Add(json_patch::AddOperation {
                        path,
                        value: serde_json::json!(kube_status),
                    })]
                }
            };

            let patch = json_patch::Patch(patches);
            let params = kube::api::PatchParams::default();
            debug!(?patch, "Patching container status");
            client
                .patch_status(pod.name(), &params, &kube::api::Patch::<()>::Json(patch))
                .await?;
            Ok(())
        }
        None => {
            warn!(
                "Container status update for unknown container {}.",
                key.name()
            );
            Ok(())
        }
    }
}

/// Create inital container status for registering pod.
pub fn make_initial_container_status(container: &Container) -> KubeContainerStatus {
    let state = ContainerState {
        waiting: Some(ContainerStateWaiting {
            message: Some("Registered".to_string()),
            reason: Some("Registered".to_string()),
        }),
        ..Default::default()
    };
    KubeContainerStatus {
        name: container.name().to_string(),
        ready: false,
        started: Some(false),
        state: Some(state),
        ..Default::default()
    }
}
