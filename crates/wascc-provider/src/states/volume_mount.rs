use crate::PodState;
use kubelet::state::prelude::*;
use kubelet::volume::Ref;

use super::error::Error;
use super::starting::Starting;
use crate::transition_to_error;

/// Kubelet is pulling container images.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(Starting, Error)]
pub struct VolumeMount;

#[async_trait::async_trait]
impl State<PodState> for VolumeMount {
    async fn next(self: Box<Self>, pod_state: &mut PodState, pod: &Pod) -> Transition<PodState> {
        pod_state.run_context.volumes = match Ref::volumes_from_pod(
            &pod_state.shared.volume_path,
            &pod,
            &pod_state.shared.client,
        )
        .await
        {
            Ok(volumes) => volumes,
            Err(e) => transition_to_error!(self, e),
        };
        Transition::next(self, Starting)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "VolumeMount")
    }
}
