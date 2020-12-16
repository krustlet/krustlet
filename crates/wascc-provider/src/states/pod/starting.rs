use std::sync::Arc;

use log::info;
use tokio::sync::RwLock;

use kubelet::container::{state::run_to_completion, ContainerKey};
use kubelet::pod::state::prelude::*;
use kubelet::state::common::error::Error;
use kubelet::state::common::GenericProviderState;

use crate::states::container::waiting::Waiting;
use crate::states::container::ContainerState;
use crate::{PodState, ProviderState};

use super::running::Running;

/// The Kubelet is starting the Pod.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(Running)]
pub struct Starting;

#[async_trait::async_trait]
impl State<PodState> for Starting {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<PodState> {
        info!("Starting containers for pod {:?}", pod.name());

        let arc_pod = Arc::new(RwLock::new(pod.clone()));

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
            let task_pod = Arc::clone(&arc_pod);
            let mut task_tx = tx.clone();
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

        info!("All containers started for pod {:?}.", pod.name());

        Transition::next(self, Running::new(rx))
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "Starting"))
    }
}

impl TransitionTo<Error<crate::WasccProvider>> for Starting {}
