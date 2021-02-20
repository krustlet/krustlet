use super::terminated::Terminated;
use super::ContainerState;
use crate::ProviderState;
use kubelet::container::state::prelude::*;
use log::{debug, warn};
use tokio::sync::mpsc::Receiver;

/// The container is starting.
#[derive(Debug, TransitionTo)]
#[transition_to(Terminated)]
pub struct Running {
    rx: Receiver<Status>,
}

impl Running {
    pub fn new(rx: Receiver<Status>) -> Self {
        Running { rx }
    }
}

#[async_trait::async_trait]
impl State<ContainerState> for Running {
    async fn next(
        mut self: Box<Self>,
        _shared_state: SharedState<ProviderState>,
        _state: &mut ContainerState,
        _container: Manifest<Container>,
    ) -> Transition<ContainerState> {
        while let Some(status) = self.rx.recv().await {
            debug!("Got status update from WASI Runtime: {:?}", &status);
            if let Status::Terminated {
                failed, message, ..
            } = status
            {
                return Transition::next(self, Terminated::new(message, failed));
            }
        }
        warn!("WASI Runtime hung up channel.");
        Transition::next(
            self,
            Terminated::new("WASI Runtime hung up channel.".to_string(), true),
        )
    }

    async fn status(
        &self,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> anyhow::Result<Status> {
        Ok(Status::running())
    }
}
