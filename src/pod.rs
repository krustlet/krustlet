use k8s_openapi::api::core::v1::{PodSpec, PodStatus};
use kube::{
    api::{Api, Object, PatchParams},
    client::APIClient,
};
use log::{error, info};

/// Alias for a Kubernetes Pod.
pub type KubePod = Object<PodSpec, PodStatus>;

/// Patch the pod status to update the phase.
pub fn pod_status(client: APIClient, pod: KubePod, phase: &str, ns: &str) {
    let status = serde_json::json!(
        {
            "metadata": {
                "resourceVersion": "",
            },
            "status": {
                "phase": phase
            }
        }
    );

    let meta = pod.metadata.clone();
    let pp = PatchParams::default();
    let data = serde_json::to_vec(&status).expect("Should always serialize");
    match Api::v1Pod(client)
        .within(ns)
        .patch_status(meta.name.as_str(), &pp, data)
    {
        Ok(o) => {
            info!("Pod status for {} set to {}", meta.name.as_str(), phase);
            info!(
                "Pod status returned: {}",
                serde_json::to_string_pretty(&o.status).unwrap()
            )
        }
        Err(e) => error!("Pod status update failed for {}: {}", meta.name.as_str(), e),
    }
}
