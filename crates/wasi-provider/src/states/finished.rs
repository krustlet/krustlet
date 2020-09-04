use crate::PodState;
use kubelet::state::prelude::*;

state!(
    /// Pod execution completed with no errors.
    Finished,
    PodState,
    { Ok(Transition::Complete(Ok(()))) },
    { make_status(Phase::Succeeded, "Finished") }
);
