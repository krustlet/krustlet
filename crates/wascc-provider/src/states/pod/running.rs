use tokio::sync::mpsc::Receiver;

use kubelet::pod::state::prelude::*;
use kubelet::state::common::error::Error;
use kubelet::state::common::GenericProviderState;

use crate::{fail_fatal, PodState, ProviderState};

/// The Kubelet is running the Pod.
#[derive(Debug, TransitionTo)]
#[transition_to()]
pub struct Running {
    rx: Receiver<anyhow::Result<()>>,
}

impl Running {
    pub fn new(rx: Receiver<anyhow::Result<()>>) -> Self {
        Running { rx }
    }
}

#[async_trait::async_trait]
impl State<PodState> for Running {
    async fn next(
        mut self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        _pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<PodState> {
        // This collects errors from registering the actor.
        if let Some(result) = self.rx.recv().await {
            match result {
                Ok(()) => {
                    // This indicates some sort of premature exit.
                    return Transition::next(
                        self,
                        Error::new(format!("Pod {} container exitted.", pod.name())),
                    );
                }
                Err(e) => {
                    // Stop remaining containers;
                    {
                        let provider = provider_state.write().await;
                        // This Result doesnt matter since we are about to exit with error.
                        provider.stop(pod).await.ok();
                    }
                    fail_fatal!(e);
                }
            }
        }
        Transition::next(
            self,
            Error::new(format!(
                "Pod {} container result channel hung up.",
                pod.name()
            )),
        )
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Running, "Running"))
    }
}

impl TransitionTo<Error<crate::WasccProvider>> for Running {}
