use super::ContainerState;
use kubelet::container::state::prelude::*;

/// The container is starting.
#[derive(Default, Debug, TransitionTo)]
#[transition_to()]
pub struct Waiting;

#[async_trait::async_trait]
impl State<ContainerState, Status> for Waiting {
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
        todo!()
        // Ok(Status::Waiting {
        // })
    }
}
