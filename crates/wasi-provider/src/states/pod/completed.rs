use crate::{PodState, ProviderState};
use kubelet::pod::state::prelude::*;

/// Pod was deleted.
#[derive(Default, Debug)]
pub struct Completed;

#[async_trait::async_trait]
impl State<PodState> for Completed {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<ProviderState>,
        _pod_state: &mut PodState,
        _pod: Manifest<Pod>,
    ) -> Transition<PodState> {
        Transition::Complete(Ok(()))
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Succeeded, "Completed"))
    }
}
