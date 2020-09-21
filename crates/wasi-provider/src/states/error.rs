use kubelet::state::prelude::*;

use super::crash_loop_backoff::CrashLoopBackoff;
use super::registered::Registered;
use crate::PodState;

#[derive(Default, Debug)]
/// The Pod failed to run.
// If we manually implement, we can allow for arguments.
pub struct Error {
    pub message: String,
}

#[async_trait::async_trait]
impl State<PodState> for Error {
    async fn next(
        self: Box<Self>,
        pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        pod_state.errors += 1;
        if pod_state.errors > 3 {
            pod_state.errors = 0;
            Ok(Transition::next(self, CrashLoopBackoff))
        } else {
            tokio::time::delay_for(std::time::Duration::from_secs(5)).await;
            Ok(Transition::next(self, Registered))
        }
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, &self.message)
    }
}

impl TransitionTo<Registered> for Error {}
impl TransitionTo<CrashLoopBackoff> for Error {}
