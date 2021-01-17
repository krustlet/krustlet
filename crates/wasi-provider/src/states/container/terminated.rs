use kubelet::container::state::prelude::*;
use log::error;
use tokio::sync::watch::Receiver;

use crate::ProviderState;

use super::ContainerState;

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
        state: &mut ContainerState,
        mut container: Receiver<Container>,
    ) -> Transition<ContainerState> {
        let container = match container.recv().await {
            Some(container) => container,
            None => return Transition::Complete(Err(anyhow::anyhow!("Manifest sender dropped."))),
        };

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
