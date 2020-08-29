use kubelet::state::{PodChangeRx, State, Transition};
use kubelet::{
    pod::{Phase, Pod, key_from_pod},
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
        let mut lock = pod_state.shared.port_map.lock().await;
        for (key, val) in lock.iter() {
            if val == pod.name() {
                delete_key = *key
            }
        }
        lock.remove(&delete_key);

        let pod_key = key_from_pod(&pod);
        {
            let mut handles = pod_state.shared.handles.write().await;
            handles.remove(&pod_key);
        }

        Ok(Transition::Complete(Ok(())))
    },
    { make_status(Phase::Succeeded, "Cleanup") }
);
