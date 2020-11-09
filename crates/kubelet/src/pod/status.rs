//! Container statuses

use super::Pod;
use crate::container::make_initial_container_status;
use k8s_openapi::api::core::v1::ContainerStatus as KubeContainerStatus;
use k8s_openapi::api::core::v1::Pod as KubePod;
use k8s_openapi::api::core::v1::PodStatus as KubePodStatus;
use kube::api::PatchParams;
use kube::Api;
use log::{debug, warn};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Patch Pod status with Kubernetes API.
pub async fn patch_status(api: &Api<KubePod>, name: &str, status: Status) {
    let patch = status.into_json();
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
            let status = make_status(
                Phase::Failed,
                "Timed out while initializing container statuses.",
            );
            patch_status(&api, &name, status).await;
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
pub fn make_registered_status(pod: &Pod) -> Status {
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
pub fn make_status(phase: Phase, reason: &str) -> Status {
    let mut status = Status::new();
    status.set_phase(phase);
    status.set_reason(reason);
    status
}

/// Create basic Pod status patch.
pub fn make_status_with_containers(
    phase: Phase,
    reason: &str,
    container_statuses: Vec<KubeContainerStatus>,
    init_container_statuses: Vec<KubeContainerStatus>,
) -> Status {
    let mut status = Status::new();
    status.set_phase(phase);
    status.set_reason(reason);
    status.set_container_statuses(container_statuses);
    status.set_init_container_statuses(init_container_statuses);
    status
}

#[derive(Clone, Debug, Default)]
/// Pod Status wrapper.
pub struct Status(KubePodStatus);

impl Status {
    /// Create a new status with no fields set.
    pub fn new() -> Self {
        Status(Default::default())
    }

    /// Set Pod phase.
    pub fn set_phase(&mut self, phase: Phase) {
        self.0.phase = Some(format!("{}", phase));
    }

    /// Set Pod reason.
    pub fn set_reason(&mut self, reason: &str) {
        self.0.reason = Some(reason.to_string());
    }

    /// Set Pod message.
    pub fn set_message(&mut self, message: &str) {
        self.0.message = Some(message.to_string());
    }

    /// Set Pod container statuses.
    pub fn set_container_statuses(&mut self, container_statuses: Vec<KubeContainerStatus>) {
        self.0.container_statuses = Some(container_statuses);
    }

    /// Set Pod init container statuses.
    pub fn set_init_container_statuses(
        &mut self,
        init_container_statuses: Vec<KubeContainerStatus>,
    ) {
        self.0.init_container_statuses = Some(init_container_statuses);
    }

    pub(crate) fn into_json(self) -> serde_json::Value {
        let mut status = serde_json::Map::new();
        if let Some(s) = self.0.phase {
            status.insert("phase".to_string(), serde_json::Value::String(s));
        };

        if let Some(s) = self.0.message {
            status.insert("message".to_string(), serde_json::Value::String(s));
        };

        if let Some(s) = self.0.reason {
            status.insert("reason".to_string(), serde_json::Value::String(s));
        };

        if let Some(s) = self.0.container_statuses {
            status.insert("containerStatuses".to_string(), serde_json::json!(s));
        };

        if let Some(s) = self.0.init_container_statuses {
            status.insert("initContainerStatuses".to_string(), serde_json::json!(s));
        };

        serde_json::json!(
            {
                "metadata": {
                    "resourceVersion": "",
                },
                "status": serde_json::Value::Object(status)
            }
        )
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

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::json!(self).as_str().unwrap())
    }
}

impl Default for Phase {
    fn default() -> Self {
        Self::Unknown
    }
}
