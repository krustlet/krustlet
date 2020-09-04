use crate::PodState;
use kubelet::state::prelude::*;

state!(
    /// Pod was deleted.
    Completed,
    PodState,
    {
        let mut lock = pod_state.shared.handles.write().await;
        if let Some(handle) = lock.get_mut(&pod_state.key) {
            handle.stop().await.unwrap()
        }
        Ok(Transition::Complete(Ok(())))
    },
    { make_status(Phase::Succeeded, "Completed") }
);
