use crate::PodState;
use kubelet::state::prelude::*;
use kubelet::volume::Ref;

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
