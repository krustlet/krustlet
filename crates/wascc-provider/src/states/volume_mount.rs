use crate::PodState;
use kubelet::state::prelude::*;
use kubelet::volume::Ref;

use super::starting::Starting;

/// Kubelet is pulling container images.
#[derive(Default, Debug)]
pub struct VolumeMount;

#[async_trait::async_trait]
impl State<PodState> for VolumeMount {
    async fn next(
        self: Box<Self>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        pod_state.run_context.volumes = Ref::volumes_from_pod(
            &pod_state.shared.volume_path,
            &pod,
            &pod_state.shared.client,
        )
        .await
        .unwrap();
        Ok(Transition::next(self, Starting))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "VolumeMount")
    }
}

impl TransitionTo<Starting> for VolumeMount {}
