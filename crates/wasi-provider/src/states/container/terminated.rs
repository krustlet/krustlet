use super::ContainerState;
use crate::ProviderState;
use kubelet::container::state::prelude::*;

/// The container is starting.
#[derive(Debug, TransitionTo)]
#[transition_to()]
pub struct Terminated {
    message: String,
    failed: bool,
}

impl Terminated {
    pub fn new(message: String, failed: bool) -> Self {
        Terminated { message, failed }
    }
}

#[async_trait::async_trait]
impl State<ContainerState> for Terminated {
    async fn next(
        self: Box<Self>,
        _shared_state: SharedState<ProviderState>,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> Transition<ContainerState> {
        if self.failed {
            Transition::Complete(Err(anyhow::anyhow!(self.message.clone())))
        } else {
            Transition::Complete(Ok(()))
        }
    }

    async fn status(
        &self,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> anyhow::Result<Status> {
        Ok(Status::terminated(&self.message, self.failed))
    }
}
