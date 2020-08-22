use kubelet::state::{PodChangeRx, State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};

use crate::{make_status, PodState};

use super::cleanup::Cleanup;

state!(
    /// Pod was deleted.
    Terminated,
    PodState,
    Cleanup,
    Terminated,
    {
        // TODO: Stop workload
        // TODO: Wait for deleted.
        Ok(Transition::Advance(Cleanup))
    },
    { make_status(Phase::Succeeded, "Terminated") }
);
