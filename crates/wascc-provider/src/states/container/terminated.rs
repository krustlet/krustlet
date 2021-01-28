use log::error;

use kubelet::container::state::prelude::*;

use crate::ProviderState;

use super::ContainerState;

/// The container is starting.
#[derive(Debug)]
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
        state: &mut ContainerState,
        container: Manifest<Container>,
    ) -> Transition<ContainerState> {
        let container = container.latest();

        if self.failed {
            error!(
                "Pod {} container {} exited with error: {}",
                state.pod.name(),
                container.name(),
                &self.message
            );
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
