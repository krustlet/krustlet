use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};
use kubelet::container::Status;
use kubelet::state::prelude::*;
use log::error;

use super::completed::Completed;
use super::error::Error;
use crate::PodState;

async fn patch_container_status(
    client: &Api<KubePod>,
    pod_name: &str,
    name: String,
    status: &Status,
) -> anyhow::Result<()> {
    // We need to fetch the current status because there is no way to merge with a strategic merge patch ere
    let mut container_statuses = match client.get(pod_name).await {
        Ok(p) => match p.status {
            Some(s) => s.container_statuses.unwrap_or_default(),
            None => {
                return Err(anyhow::anyhow!(
                    "Pod is missing status information. This should not occur"
                ));
            }
        },
        Err(e) => {
            error!("Unable to fetch current status of pod {}, aborting status patch (will be retried on next status update): {:?}", pod_name, e);
            // FIXME: This is kinda...ugly. But we can't just
            // randomly abort the whole process due to an error
            // fetching the current status. We should probably have
            // some sort of retry mechanism, but that is another
            // task for another day
            Vec::default()
        }
    };
    match container_statuses.iter().position(|s| s.name == name) {
        Some(i) => {
            container_statuses[i] = status.to_kubernetes(name);
        }
        None => {
            container_statuses.push(status.to_kubernetes(name));
        }
    };
    let s = serde_json::json!({
        "metadata": {
            "resourceVersion": "",
        },
        "status": {
            "containerStatuses": container_statuses,
        }
    });
    client
        .patch_status(pod_name, &PatchParams::default(), serde_json::to_vec(&s)?)
        .await?;
    Ok(())
}

/// The Kubelet is running the Pod.
#[derive(Default, Debug)]
pub struct Running;

#[async_trait::async_trait]
impl State<PodState> for Running {
    async fn next(
        self: Box<Self>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        let client: Api<KubePod> = Api::namespaced(
            kube::Client::new(pod_state.shared.kubeconfig.clone()),
            pod.namespace(),
        );
        let mut completed = 0;
        let total_containers = pod.containers().len();

        while let Some((name, status)) = pod_state.run_context.status_recv.recv().await {
            // TODO: implement a container state machine such that it will self-update the Kubernetes API as it transitions through these stages.
            if let Err(e) = patch_container_status(&client, &pod.name(), name, &status).await {
                error!("Unable to patch status, will retry on next update: {:?}", e);
            }
            if let Status::Terminated {
                timestamp: _,
                message,
                failed,
            } = status
            {
                if failed {
                    return Ok(Transition::next(self, Error { message }));
                } else {
                    completed += 1;
                    if completed == total_containers {
                        return Ok(Transition::next(self, Completed));
                    }
                }
            }
        }
        Ok(Transition::next(self, Completed))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Running, "Running")
    }
}

impl TransitionTo<Completed> for Running {}
impl TransitionTo<Error> for Running {}
