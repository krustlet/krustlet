use crate::{PodState, ProviderState};
use kubelet::state::common::error::Error;
use kubelet::state::prelude::*;
use kubelet::volume::Ref;

use super::initializing::Initializing;
use crate::transition_to_error;

/// Kubelet is pulling container images.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(Initializing)]
pub struct VolumeMount;

#[async_trait::async_trait]
impl State<ProviderState, PodState> for VolumeMount {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<ProviderState, PodState> {
        let client = kube::Client::new(pod_state.shared.kubeconfig.clone());
        pod_state.run_context.volumes =
            match Ref::volumes_from_pod(&pod_state.shared.volume_path, &pod, &client).await {
                Ok(volumes) => volumes,
                Err(e) => transition_to_error!(self, e),
            };
        Transition::next(self, Initializing)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "VolumeMount")
    }
}

impl TransitionTo<Error<crate::WasiProvider>> for VolumeMount {}
