use kubelet::state::{State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};

use crate::{make_status, PodState};

state!(
    /// Pod execution completed with no errors.
    Finished,
    PodState,
    { Ok(Transition::Complete(Ok(()))) },
    { make_status(Phase::Succeeded, "Finished") }
);
