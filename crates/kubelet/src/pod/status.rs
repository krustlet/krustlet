//! Container statuses

use std::collections::HashMap;

use k8s_openapi::api::core::v1::Pod;
use kube::{api::PatchParams, Api};

use crate::container::Status as ContainerStatus;

/// Describe the status of a workload.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Allows a provider to set a custom message, otherwise, kubelet will infer
    /// a message from the container statuses
    pub message: Option<String>,
    /// The statuses of containers keyed off their names
    pub container_statuses: HashMap<String, ContainerStatus>,
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
