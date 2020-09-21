use crate::PodState;
use kubelet::state::prelude::*;

/// The Kubelet is running the Pod.
#[derive(Default, Debug)]
pub struct Running;

#[async_trait::async_trait]
impl State<PodState> for Running {
    async fn next(
        self: Box<Self>,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        // Wascc has no notion of exiting so we just sleep.
        // I _think_ that periodically awaiting will allow the task to be interrupted.
        loop {
            tokio::time::delay_for(std::time::Duration::from_secs(10)).await;
        }
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Running, "Running")
    }
}
