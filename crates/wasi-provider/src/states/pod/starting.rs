use std::sync::Arc;

use tracing::{info, instrument};

use kubelet::container::state::run_to_completion;
use kubelet::container::ContainerKey;
use kubelet::pod::state::prelude::*;
use kubelet::state::common::GenericProviderState;

use crate::states::container::waiting::Waiting;
use crate::states::container::ContainerState;
use crate::{PodState, ProviderState};

use super::running::Running;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(Running)]
/// The Kubelet is starting the Pod containers
pub(crate) struct Starting;

#[async_trait::async_trait]
impl State<PodState> for Starting {
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

        info!("Starting containers for pod");
        let containers = pod.containers();
        let (tx, rx) = tokio::sync::mpsc::channel(containers.len());
        for container in containers {
            let initial_state = Waiting;
            let container_key = ContainerKey::App(container.name().to_string());
            let container_state = ContainerState::new(
                pod.clone(),
                container_key.clone(),
                Arc::clone(&pod_state.run_context),
            );
            let task_provider = Arc::clone(&provider_state);
            let task_tx = tx.clone();
            let task_pod = pod_rx.clone();
            tokio::task::spawn(async move {
                let client = {
                    let provider_state = task_provider.read().await;
                    provider_state.client()
                };

                let result = run_to_completion(
                    &client,
                    initial_state,
                    task_provider,
                    container_state,
                    task_pod,
                    container_key,
                )
                .await;
                task_tx.send(result).await
            });
        }
        info!("All containers started for pod");
        Transition::next(self, Running::new(rx))
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "Starting"))
    }
}
