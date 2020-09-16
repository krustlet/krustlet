use crate::PodState;
use kubelet::state::prelude::*;

use super::registered::Registered;

state!(
    /// Pod has failed multiple times.
    CrashLoopBackoff,
    PodState,
    {
        tokio::time::delay_for(std::time::Duration::from_secs(60)).await;
        Ok(Transition::Advance(Box::new(Registered)))
    },
    { make_status(Phase::Pending, "CrashLoopBackoff") }
);
