use crate::PodState;
use kubelet::state::prelude::*;

use super::registered::Registered;

#[derive(Debug)]
pub struct CrashLoopBackoff;

#[async_trait::async_trait]
impl State<PodState> for CrashLoopBackoff {
    async fn next(
        self: Box<Self>,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        tokio::time::delay_for(std::time::Duration::from_secs(60)).await;
        Ok(Transition::next(self, Registered))
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
