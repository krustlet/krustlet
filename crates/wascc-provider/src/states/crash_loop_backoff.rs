use crate::PodState;
use kubelet::backoff::BackoffStrategy;
use kubelet::state::prelude::*;

use super::registered::Registered;

/// Pod has failed multiple times.
#[derive(Default, Debug)]
pub struct CrashLoopBackoff;

#[async_trait::async_trait]
impl State<PodState> for CrashLoopBackoff {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        pod_state.crash_loop_backoff_strategy.wait().await;
        Transition::next(self, Registered)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "CrashLoopBackoff")
    }
}

impl TransitionTo<Registered> for CrashLoopBackoff {}
