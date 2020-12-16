use super::terminated::Terminated;
use super::ContainerState;
use crate::ProviderState;
use kubelet::container::state::prelude::*;

/// The container is starting.
#[derive(Debug, TransitionTo)]
#[transition_to(Terminated)]
pub struct Running;

#[async_trait::async_trait]
impl State<ContainerState> for Running {
    async fn next(
        mut self: Box<Self>,
        _shared_state: SharedState<ProviderState>,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> Transition<ContainerState> {
        loop {
            tokio::time::delay_for(std::time::Duration::from_secs(10)).await;
        }
    }

    async fn status(
        &self,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> anyhow::Result<Status> {
        Ok(Status::running())
    }
}
