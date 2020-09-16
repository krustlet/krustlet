use crate::PodState;
use kubelet::state::prelude::*;

state!(
    /// The Kubelet is running the Pod.
    Running,
    PodState,
    {
        // Wascc has no notion of exiting so we just sleep.
        // I _think_ that periodically awaiting will allow the task to be interrupted.
        loop {
            tokio::time::delay_for(std::time::Duration::from_secs(10)).await;
        }
    },
    { make_status(Phase::Running, "Running") }
);
