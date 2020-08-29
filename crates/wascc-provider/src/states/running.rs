use kubelet::state::{State, Transition};
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
        // Wascc has no notion of exiting so we just sleep.
        // I _think_ that periodically awaiting will allow the task to be interrupted.
        loop {
            tokio::time::delay_for(std::time::Duration::from_secs(10)).await;
        }
    },
    { make_status(Phase::Running, "Running") }
);
