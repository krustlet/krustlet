use kubelet::state::{State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};

use crate::{make_status, PodState};

state!(
    /// Pod was deleted.
    Terminated,
    PodState,
    Terminated,
    Terminated,
    {
        let mut lock = pod_state.shared.handles.write().await;
        if let Some(handle) = lock.get_mut(&pod_state.key) {
            handle.stop().await.unwrap()
        }
        Ok(Transition::Complete(Ok(())))
    },
    { make_status(Phase::Succeeded, "Terminated") }
);
