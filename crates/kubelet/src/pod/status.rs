//! Container statuses

use super::Pod;
use crate::container::{make_initial_container_status, ContainerMap, Status as ContainerStatus};
use k8s_openapi::api::core::v1::ContainerStatus as KubeContainerStatus;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::PatchParams;
use kube::Api;
use log::{debug, warn};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Patch Pod status with Kubernetes API.
pub async fn patch_status(api: &Api<KubePod>, name: &str, patch: serde_json::Value) {
    match serde_json::to_vec(&patch) {
        Ok(data) => {
            debug!(
                "Applying status patch to Pod {}: '{}'",
                &name,
                std::str::from_utf8(&data).unwrap()
            );
            match api.patch_status(&name, &PatchParams::default(), data).await {
                Ok(_) => (),
                Err(e) => {
                    warn!("Pod {} error patching status: {:?}", name, e);
                }
            }
        }
        Err(e) => {
            warn!(
                "Pod {} error serializing status patch {:?}: {:?}",
                name, &patch, e
            );
        }
    }
}

const MAX_STATUS_INIT_RETRIES: usize = 5;

/// Initializes Pod container status array and wait for Pod reflection to update.
pub async fn initialize_pod_container_statuses(
    name: &str,
    pod: Arc<RwLock<Pod>>,
    api: &Api<KubePod>,
) -> anyhow::Result<()> {
    // NOTE: This loop patches the container statuses of the Pod with and then
    // waits for them to be picked up by the reflector. This is needed for a
    // few reasons:
    // * Kubernetes rewrites an empty array to null, preventing us from
    //   starting with that and appending.
    // * Pod reflection is not updated within a given state, meaning that
    //   container status patching cannot be responsible for initializing this
    //   (this would be a race condition anyway).
    // I'm not sure if we want to loop forever or handle some sort of failure
    // condition (if Kubernetes refuses to accept and propagate this
    // initialization patch.)
    let mut retries = 0;
    'main: loop {
        if retries == MAX_STATUS_INIT_RETRIES {
            let patch = serde_json::json!(
                {
                    "metadata": {
                        "resourceVersion": "",
                    },
                    "status": {
                        "phase": Phase::Failed,
                        "reason": "Timed out while initializing container statuses.",
                    }
                }
            );
            patch_status(&api, &name, patch).await;
            anyhow::bail!("Timed out while initializing container statuses.")
        }
        let (num_containers, num_init_containers) = {
            let pod = pod.read().await;
            patch_status(&api, &name, make_registered_status(&pod)).await;
            let num_containers = pod.containers().len();
            let num_init_containers = pod.init_containers().len();
            (num_containers, num_init_containers)
        };
        for _ in 0..10 {
            let status = {
                pod.read()
                    .await
                    .as_kube_pod()
                    .status
                    .clone()
                    .unwrap_or_default()
            };

            let num_statuses = status
                .container_statuses
                .as_ref()
                .map(|statuses| statuses.len())
                .unwrap_or(0);
            let num_init_statuses = status
                .init_container_statuses
                .as_ref()
                .map(|statuses| statuses.len())
                .unwrap_or(0);

            if (num_statuses == num_containers) && (num_init_statuses == num_init_containers) {
                break 'main Ok(());
            } else {
                debug!("Pod {} waiting for status to populate: {:?}", &name, status);
                tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
            }
        }
        retries += 1;
    }
}

/// Initialize Pod status.
/// This initializes Pod status to include containers in the correct order as expected by
/// `patch_container_status`.
pub fn make_registered_status(pod: &Pod) -> serde_json::Value {
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
) -> serde_json::Value {
    serde_json::json!(
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
    )
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
