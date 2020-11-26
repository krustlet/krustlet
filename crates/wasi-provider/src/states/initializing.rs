use std::collections::HashMap;

use log::{error, info};

use crate::{PodState, ProviderState};
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};
use kubelet::backoff::BackoffStrategy;
use kubelet::container::{patch_container_status, ContainerKey, Status as ContainerStatus};
use kubelet::pod::state::prelude::*;
use kubelet::pod::{Handle, PodKey};
use kubelet::state::common::error::Error;
use kubelet::state::common::GenericProviderState;
use kubelet::state::prelude::*;

use super::starting::{start_container, ContainerHandleMap, Starting};
use crate::fail_fatal;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(Starting)]
pub struct Initializing;

#[async_trait::async_trait]
impl State<ProviderState, PodState> for Initializing {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<ProviderState, PodState> {
        let client: Api<KubePod> =
            Api::namespaced(provider_state.read().await.client(), pod.namespace());
        let mut container_handles: ContainerHandleMap = HashMap::new();

        for init_container in pod.init_containers() {
            info!(
                "Starting init container {:?} for pod {:?}",
                init_container.name(),
                pod.name()
            );

            // Each new init container resets the CrashLoopBackoff timer.
            pod_state.crash_loop_backoff_strategy.reset();

            match start_container(&provider_state, pod_state, pod, &init_container).await {
                Ok(h) => {
                    container_handles
                        .insert(ContainerKey::Init(init_container.name().to_string()), h);
                }
                Err(e) => fail_fatal!(e),
            }

            while let Some((name, status)) = pod_state.run_context.status_recv.recv().await {
                if let Err(e) = patch_container_status(
                    &client,
                    &pod,
                    &ContainerKey::Init(name.clone()),
                    &status,
                )
                .await
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
                        let pod_handle = Handle::new(container_handles, pod.clone(), None);
                        let pod_key = PodKey::from(pod);
                        {
                            let state_writer = provider_state.write().await;
                            let mut handles = state_writer.handles.write().await;
                            handles.insert(pod_key, pod_handle);
                        }

                        let status_json = match serde_json::to_vec(&s) {
                            Ok(json) => json,
                            Err(e) => fail_fatal!(e),
                        };

                        match client
                            .patch_status(pod.name(), &PatchParams::default(), status_json)
                            .await
                        {
                            Ok(_) => return Transition::next(self, Error::<_>::new(message)),
                            Err(e) => fail_fatal!(e),
                        };
                    } else {
                        break;
                    }
                }
            }
        }
        info!("Finished init containers for pod {:?}", pod.name());
        pod_state.crash_loop_backoff_strategy.reset();
        Transition::next(self, Starting::new(container_handles))
    }

    async fn status(
        &self,
        _pod_state: &mut PodState,
        _pmeod: &Pod,
    ) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Running, "Initializing"))
    }
}

impl TransitionTo<Error<crate::WasiProvider>> for Initializing {}
