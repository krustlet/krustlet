use crate::PodState;
use kubelet::state::prelude::*;

/// Pod execution completed with no errors.
#[derive(Default, Debug)]
pub struct Finished;

#[async_trait::async_trait]
impl State<PodState> for Finished {
    async fn next(
        self: Box<Self>,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        Ok(Transition::Complete(Ok(())))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Succeeded, "Finished")
    }
}
