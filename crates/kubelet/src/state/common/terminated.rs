//! Pod was deleted.

use super::{GenericProvider, GenericProviderState};
use crate::pod::state::prelude::*;

/// Pod was deleted.
pub struct Terminated<P: GenericProvider> {
    phantom: std::marker::PhantomData<P>,
}

impl<P: GenericProvider> std::fmt::Debug for Terminated<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "Terminated".fmt(formatter)
    }
}

impl<P: GenericProvider> Default for Terminated<P> {
    fn default() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<P: GenericProvider> State<P::PodState> for Terminated<P> {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<P::ProviderState>,
        _pod_state: &mut P::PodState,
        pod: Manifest<Pod>,
    ) -> Transition<P::PodState> {
        let pod = pod.latest();

        let state_reader = provider_state.read().await;
        // TODO: In original code, pod key was stored in state rather than
        // re-derived.  Is this important e.g. could pod mutate in ways
        // that invalidate the key assigned on startup?
        let stop_result = state_reader.stop(&pod).await;
        Transition::Complete(stop_result)
    }

    async fn status(&self, _pod_state: &mut P::PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Succeeded, "Terminated"))
    }
}
