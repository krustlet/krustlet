use kubelet::state::{PodChangeRx, State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};

use crate::{make_status, PodState};

use super::error::Error;
use super::finished::Finished;

state!(
    /// The Kubelet is running the Pod.
    Running,
    PodState,
    Finished,
    Error,
    {
        // Listen for pod changes.
        // Wait for execution to complete.
        Ok(Transition::Advance(Finished))
    },
    { make_status(Phase::Running, "Running") }
);
