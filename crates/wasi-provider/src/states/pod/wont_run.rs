use crate::PodState;
use crate::ProviderState;
use kubelet::pod::state::prelude::*;

/// The Kubelet is ignoring this Pod.
#[derive(Default, Debug)]
pub struct WontRun;

#[async_trait::async_trait]
impl State<PodState> for WontRun {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<ProviderState>,
        _state: &mut PodState,
        _pod: &Pod,
    ) -> Transition<PodState> {
        loop {
            tokio::time::delay_for(std::time::Duration::from_secs(60)).await;
        }
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "WontRun"))
    }
}
