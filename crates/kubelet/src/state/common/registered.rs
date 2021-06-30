//! The Kubelet is aware of the Pod.

use crate::pod::state::prelude::*;
use tracing::{debug, error, info, instrument};

use super::error::Error;
use super::resources::Resources;
use super::GenericProvider;

/// The Kubelet is aware of the Pod.
pub struct Registered<P: GenericProvider> {
    phantom: std::marker::PhantomData<P>,
}

impl<P: GenericProvider> std::fmt::Debug for Registered<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "Registered".fmt(formatter)
    }
}

impl<P: GenericProvider> Default for Registered<P> {
    fn default() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<P: GenericProvider> State<P::PodState> for Registered<P> {
    #[instrument(
        level = "info",
        skip(self, _provider_state, _pod_state, pod),
        fields(pod_name)
    )]
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<P::ProviderState>,
        _pod_state: &mut P::PodState,
        pod: Manifest<Pod>,
    ) -> Transition<P::PodState> {
        let pod = pod.latest();

        tracing::Span::current().record("pod_name", &pod.name());

        debug!("Preparing to register pod");
        match P::validate_pod_and_containers_runnable(&pod) {
            Ok(_) => (),
            Err(e) => {
                error!(error = %e);
                let next = Error::<P>::new(e.to_string());
                return Transition::next(self, next);
            }
        }
        info!("Pod registered");
        let next = Resources::<P>::default();
        Transition::next(self, next)
    }

    async fn status(&self, _pod_state: &mut P::PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "Registered"))
    }
}

impl<P: GenericProvider> TransitionTo<Error<P>> for Registered<P> {}
impl<P: GenericProvider> TransitionTo<Resources<P>> for Registered<P> {}
