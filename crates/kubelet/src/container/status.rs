use crate::container::Container;
use crate::pod::Pod;
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateRunning, ContainerStateTerminated, ContainerStateWaiting,
    ContainerStatus as KubeContainerStatus, Pod as KubePod,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use log::warn;

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
pub async fn patch_container_status(
    client: &kube::Api<KubePod>,
    pod: &Pod,
    container_name: &str,
    status: &Status,
    init: bool,
) -> anyhow::Result<()> {
    let containers: Vec<Container> = if init {
        pod.init_containers()
    } else {
        pod.containers()
    };
    match containers
        .iter()
        .enumerate()
        .find(|(_, container)| container.name() == container_name)
    {
        Some((container_index, container)) => {
            let mut patches: Vec<json_patch::PatchOperation> = Vec::with_capacity(3);
            let kube_status = status.to_kubernetes(container.name());
            let path_prefix = if init {
                format!("/status/initContainerStatuses/{}", container_index)
            } else {
                format!("/status/containerStatuses/{}", container_index)
            };

            patches.push(json_patch::PatchOperation::Replace(
                json_patch::ReplaceOperation {
                    path: "/metadata/resourceVersion".to_string(),
                    value: serde_json::json!(""),
                },
            ));

            patches.push(json_patch::PatchOperation::Replace(
                json_patch::ReplaceOperation {
                    path: format!("{}/state", path_prefix),
                    value: serde_json::json!(kube_status.state.unwrap()),
                },
            ));
            patches.push(json_patch::PatchOperation::Replace(
                json_patch::ReplaceOperation {
                    path: format!("{}/ready", path_prefix),
                    value: serde_json::json!(kube_status.ready),
                },
            ));
            patches.push(json_patch::PatchOperation::Replace(
                json_patch::ReplaceOperation {
                    path: format!("{}/started", path_prefix),
                    value: serde_json::json!(true),
                },
            ));

            let patch = json_patch::Patch(patches);
            let mut params = kube::api::PatchParams::default();
            params.patch_strategy = kube::api::PatchStrategy::JSON;
            client
                .patch_status(pod.name(), &params, serde_json::to_vec(&patch)?)
                .await?;
            Ok(())
        }
        None => {
            warn!(
                "Container status update for unknown container {}.",
                container_name
            );
            Ok(())
        }
    }
}

/// Create inital container status for registering pod.
pub fn make_initial_container_status(container: &Container) -> KubeContainerStatus {
    // Create empty patch and update only the fields we want to change.
    let mut status: KubeContainerStatus = Default::default();
    let mut state: ContainerState = Default::default();
    status.name = container.name().to_string();
    status.ready = false;
    status.started = Some(false);
    state.waiting = Some(ContainerStateWaiting {
        message: Some("Registered".to_string()),
        reason: Some("Registered".to_string()),
    });
    status.state = Some(state);
    status
}
