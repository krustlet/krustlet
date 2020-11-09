use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::Api;
use kubelet::container::patch_container_status;
use kubelet::container::{ContainerKey, Status};
use kubelet::state::common::error::Error;
use kubelet::state::common::GenericProviderState;
use kubelet::pod::state::prelude::*;
use log::error;

use super::completed::Completed;
use crate::fail_fatal;
use crate::{PodState, ProviderState};

/// The Kubelet is running the Pod.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(Completed)]
pub struct Running;

#[async_trait::async_trait]
impl State<ProviderState, PodState> for Running {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<ProviderState, PodState> {
        let client: Api<KubePod> =
            Api::namespaced(provider_state.read().await.client(), pod.namespace());
        let mut completed = 0;
        let total_containers = pod.containers().len();

        while let Some((name, status)) = pod_state.run_context.status_recv.recv().await {
            // TODO: implement a container state machine such that it will self-update the Kubernetes API as it transitions through these stages.

            if let Err(e) =
                patch_container_status(&client, &pod, &ContainerKey::App(name.clone()), &status)
                    .await
            {
                error!("Unable to patch status, will retry on next update: {:?}", e);
            }

            if let Status::Terminated {
                timestamp: _,
                message,
                failed,
            } = status
            {
                if failed {
                    // This appears to be required by the test `test_module_exiting_with_error`
                    let e = anyhow::anyhow!(message);
                    fail_fatal!(e);
                // return Transition::next(self, Error { message });
                } else {
                    completed += 1;
                    if completed == total_containers {
                        return Transition::next(self, Completed);
                    }
                }
            }
        }
        Transition::next(self, Completed)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Running, "Running"))
    }
}

impl TransitionTo<Error<crate::WasiProvider>> for Running {}
