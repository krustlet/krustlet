use std::sync::Arc;

use tracing::{error, info, instrument};

use kubelet::backoff::BackoffStrategy;
use kubelet::container::state::run_to_completion;
use kubelet::container::ContainerKey;
use kubelet::pod::state::prelude::*;
use kubelet::state::common::error::Error;
use kubelet::state::common::GenericProviderState;

use crate::states::container::waiting::Waiting;
use crate::states::container::ContainerState;
use crate::{PodState, ProviderState};

use super::starting::Starting;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(Starting, Error<crate::WasiProvider>)]
pub struct Initializing;

#[async_trait::async_trait]
impl State<PodState> for Initializing {
    #[instrument(
        level = "info",
        skip(self, provider_state, pod_state, pod),
        fields(pod_name)
    )]
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: Manifest<Pod>,
    ) -> Transition<PodState> {
        let pod_rx = pod.clone();
        let pod = pod.latest();

        tracing::Span::current().record("pod_name", &pod.name());

        let client = {
            let provider_state = provider_state.read().await;
            provider_state.client()
        };

        for init_container in pod.init_containers() {
            info!(
                container_name = init_container.name(),
                "Starting init container for pod"
            );

            // Each new init container resets the CrashLoopBackoff timer.
            pod_state.crash_loop_backoff_strategy.reset();

            let initial_state = Waiting;

            let container_key = ContainerKey::Init(init_container.name().to_string());
            let container_state = ContainerState::new(
                pod.clone(),
                container_key.clone(),
                Arc::clone(&pod_state.run_context),
            );

            match run_to_completion(
                &client,
                initial_state,
                // TODO: I think everything should be a SharedState to the same pod in the reflector.
                Arc::clone(&provider_state),
                container_state,
                pod_rx.clone(),
                container_key,
            )
            .await
            {
                Ok(_) => (),
                Err(e) => {
                    error!(error = %e, "Init container failed");
                    return Transition::Complete(Err(anyhow::anyhow!(format!(
                        "Init container {} failed",
                        init_container.name()
                    ))));
                }
            }
        }
        info!("Finished init containers for pod");
        pod_state.crash_loop_backoff_strategy.reset();
        Transition::next(self, Starting)
    }

    async fn status(&self, _pod_state: &mut PodState, _pmeod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Running, "Initializing"))
    }
}
