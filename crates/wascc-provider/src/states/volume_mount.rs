use kubelet::state::{State, Transition};
use kubelet::volume::Ref;
use kubelet::{
    pod::{Phase, Pod},
    state,
};

use crate::{make_status, PodState};

use super::starting::Starting;

state!(
    /// Kubelet is pulling container images.
    VolumeMount,
    PodState,
    {
        pod_state.run_context.volumes = Ref::volumes_from_pod(
            &pod_state.shared.volume_path,
            &pod,
            &pod_state.shared.client,
        )
        .await
        .unwrap();
        Ok(Transition::Advance(Box::new(Starting)))
    },
    { make_status(Phase::Pending, "VolumeMount") }
);
