use kubelet::state::{PodChangeRx, State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};

use crate::{make_status, PodState};

state!(
    /// Clean up any pod resources..
    Cleanup,
    PodState,
    Cleanup,
    Cleanup,
    {
        let mut delete_key: i32 = 0;
        let mut lock = pod_state.port_map.lock().await;
        for (key, val) in lock.iter() {
            if val == pod.name() {
                delete_key = *key
            }
        }
        lock.remove(&delete_key);

        Ok(Transition::Complete(Ok(())))
    },
    { make_status(Phase::Succeeded, "Cleanup") }
);
