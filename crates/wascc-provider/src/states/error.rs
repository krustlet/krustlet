use kubelet::pod::{Phase, Pod};
use kubelet::state::{State, Transition};

use super::crash_loop_backoff::CrashLoopBackoff;
use super::registered::Registered;
use crate::{make_status, PodState};

#[derive(Default, Debug)]
/// The Pod failed to run.
// If we manually implement, we can allow for arguments.
pub struct Error {
    pub message: String,
}

#[async_trait::async_trait]
impl State<PodState> for Error {
    async fn next(
        &self,
        pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<Box<dyn State<PodState>>, Box<dyn State<PodState>>>> {
        pod_state.errors += 1;
        if pod_state.errors > 3 {
            pod_state.errors = 0;
            Ok(Transition::Error(Box::new(CrashLoopBackoff)))
        } else {
            tokio::time::delay_for(std::time::Duration::from_secs(5)).await;
            Ok(Transition::Advance(Box::new(Registered)))
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
