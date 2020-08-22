use kubelet::state::{PodChangeRx, State, Transition};
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
    Registered,
    CrashLoopBackoff,
    {
        // TODO: Handle pod delete?
        tokio::time::delay_for(std::time::Duration::from_secs(60)).await;
        Ok(Transition::Advance(Registered))
    },
    { make_status(Phase::Pending, "CrashLoopBackoff") }
);
