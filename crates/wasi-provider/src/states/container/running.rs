use super::terminated::Terminated;
use super::ContainerState;
use kubelet::container::state::prelude::*;

/// The container is starting.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(Terminated)]
pub struct Running;

#[async_trait::async_trait]
impl State<ContainerState> for Running {
    async fn next(
        self: Box<Self>,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> Transition<ContainerState> {
        todo!()
    }

    async fn status(
        &self,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> anyhow::Result<Status> {
        Ok(Status::running())
    }
}
