use crate::{PodState, ProviderState};
use kubelet::state::prelude::*;

/// Pod was deleted.
#[derive(Default, Debug)]
pub struct Completed;

#[async_trait::async_trait]
impl State<ProviderState, PodState> for Completed {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<ProviderState>,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> Transition<ProviderState, PodState> {
        Transition::Complete(Ok(()))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Succeeded, "Completed")
    }
}
