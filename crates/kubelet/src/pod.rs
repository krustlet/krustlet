use kube::{
    api::{Api, PatchParams},
    client::APIClient,
};
use log::{debug, error, info};

pub type Pod = k8s_openapi::api::core::v1::Pod;

/// Patch the pod status to update the phase.
pub async fn pod_status(client: APIClient, pod: &Pod, phase: &str, ns: &str) {
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

    let meta = pod.metadata.as_ref();
    let data = serde_json::to_vec(&status).expect("Should always serialize");
    let name = meta.and_then(|m| m.name.as_deref()).unwrap_or_default();
    let api: Api<Pod> = Api::namespaced(client, ns);
    match api.patch_status(&name, &PatchParams::default(), data).await {
        Ok(o) => {
            info!("Pod status for {} set to {}", name, phase);
            debug!("Pod status returned: {:#?}", o.status)
        }
        Err(e) => error!("Pod status update failed for {}: {}", name, e),
    }
}
