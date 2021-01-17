//! Kubelet is pulling container images.

use log::error;

use super::{GenericPodState, GenericProvider, GenericProviderState};
use crate::pod::state::prelude::*;
use crate::state::common::error::Error;
use crate::volume::Ref;

/// Kubelet is pulling container images.
pub struct VolumeMount<P: GenericProvider> {
    phantom: std::marker::PhantomData<P>,
}

impl<P: GenericProvider> std::fmt::Debug for VolumeMount<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "VolumeMount".fmt(formatter)
    }
}

impl<P: GenericProvider> Default for VolumeMount<P> {
    fn default() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<P: GenericProvider> State<P::PodState> for VolumeMount<P> {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<P::ProviderState>,
        pod_state: &mut P::PodState,
        mut pod: Receiver<Pod>,
    ) -> Transition<P::PodState> {
        let pod = match pod.recv().await {
            Some(pod) => pod,
            None => return Transition::Complete(Err(anyhow::anyhow!("Manifest sender dropped."))),
        };

        let (client, volume_path) = {
            let state_reader = provider_state.read().await;
            (state_reader.client(), state_reader.volume_path())
        };
        let volumes = match Ref::volumes_from_pod(&volume_path, &pod, &client).await {
            Ok(v) => v,
            Err(e) => {
                error!("{:?}", e);
                let next = Error::<P>::new(e.to_string());
                return Transition::next(self, next);
            }
        };
        pod_state.set_volumes(volumes).await;
        Transition::next_unchecked(self, P::RunState::default())
    }

    async fn status(&self, _pod_state: &mut P::PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "VolumeMount"))
    }
}

impl<P: GenericProvider> TransitionTo<Error<P>> for VolumeMount<P> {}
