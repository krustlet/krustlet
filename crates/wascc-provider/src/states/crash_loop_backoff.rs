use kubelet::state::{State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};

use crate::{make_status, PodState};

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
