use crate::PodState;
use kubelet::state::prelude::*;
use kubelet::volume::Ref;

use super::initializing::Initializing;

state!(
    /// Kubelet is pulling container images.
    VolumeMount,
    PodState,
    {
        let client = kube::Client::new(pod_state.shared.kubeconfig.clone());
        pod_state.run_context.volumes =
            Ref::volumes_from_pod(&pod_state.shared.volume_path, &pod, &client)
                .await
                .unwrap();
        Ok(Transition::Advance(Box::new(Initializing)))
    },
    { make_status(Phase::Pending, "VolumeMount") }
);
