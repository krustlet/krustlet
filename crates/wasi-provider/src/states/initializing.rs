use std::collections::HashMap;

use log::{error, info};

use crate::PodState;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};
use kubelet::container::{ContainerKey, Status as ContainerStatus};
use kubelet::pod::{key_from_pod, Handle};
use kubelet::state::prelude::*;

use super::error::Error;
use super::starting::{start_container, ContainerHandleMap, Starting};

async fn patch_init_status(
    client: &Api<KubePod>,
    pod_name: &str,
    name: String,
    status: &ContainerStatus,
) -> anyhow::Result<()> {
    // We need to fetch the current status because there is no way to merge with a strategic merge patch here
    let mut init_container_statuses = match client.get(pod_name).await {
        Ok(p) => match p.status {
            Some(s) => s.init_container_statuses.unwrap_or_default(),
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
    match init_container_statuses.iter().position(|s| s.name == name) {
        Some(i) => {
            init_container_statuses[i] = status.to_kubernetes(name);
        }
        None => {
            init_container_statuses.push(status.to_kubernetes(name));
        }
    };
    let s = serde_json::json!({
        "metadata": {
            "resourceVersion": "",
        },
        "status": {
            "initContainerStatuses": init_container_statuses,
        }
    });
    client
        .patch_status(pod_name, &PatchParams::default(), serde_json::to_vec(&s)?)
        .await?;
    Ok(())
}

#[derive(Debug)]
pub struct Initializing;

#[async_trait::async_trait]
impl State<PodState> for Initializing {
    async fn next(
        self: Box<Self>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        let client: Api<KubePod> = Api::namespaced(
            kube::Client::new(pod_state.shared.kubeconfig.clone()),
            pod.namespace(),
        );
        let mut container_handles: ContainerHandleMap = HashMap::new();

        for init_container in pod.init_containers() {
            info!(
                "Starting init container {:?} for pod {:?}",
                init_container.name(),
                pod.name()
            );

            let handle = start_container(pod_state, pod, &init_container).await?;

            container_handles.insert(
                ContainerKey::Init(init_container.name().to_string()),
                handle,
            );

            while let Some((name, status)) = pod_state.run_context.status_recv.recv().await {
                if let Err(e) = patch_init_status(&client, &pod.name(), name.clone(), &status).await
                {
                    error!("Unable to patch status, will retry on next update: {:?}", e);
                }
                if let ContainerStatus::Terminated {
                    timestamp: _,
                    message,
                    failed,
                } = status
                {
                    if failed {
                        // HACK: update the status message informing which init container failed
                        let s = serde_json::json!({
                            "metadata": {
                                "resourceVersion": "",
                            },
                            "status": {
                                "message": format!("Init container {} failed", name),
                            }
                        });

                        // If we are in a failed state, insert in the init containers we already ran
                        // into a pod handle so they are available for future log fetching
                        let pod_handle = Handle::new(container_handles, pod.clone(), None).await?;
                        let pod_key = key_from_pod(&pod);
                        {
                            let mut handles = pod_state.shared.handles.write().await;
                            handles.insert(pod_key, pod_handle);
                        }
                        client
                            .patch_status(
                                pod.name(),
                                &PatchParams::default(),
                                serde_json::to_vec(&s)?,
                            )
                            .await?;
                        return Ok(Transition::next(self, Error { message }));
                    } else {
                        break;
                    }
                }
            }
        }
        info!("Finished init containers for pod {:?}", pod.name());
        Ok(Transition::next(self, Starting::new(container_handles)))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pmeod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Running, "Initializing")
    }
}

impl TransitionTo<Error> for Initializing {}
impl TransitionTo<Starting> for Initializing {}
