//! The Kubelet is aware of the Pod.

use crate::state::prelude::*;

use log::{debug, error, info};

use super::error::Error;
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
impl<P: GenericProvider> State<P::ProviderState, P::PodState> for Registered<P> {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<P::ProviderState>,
        _pod_state: &mut P::PodState,
        pod: &Pod,
    ) -> Transition<P::ProviderState, P::PodState> {
        debug!("Preparing to register pod: {}", pod.name());
        match P::validate_pod_and_containers_runnable(&pod) {
            Ok(_) => (),
            Err(e) => {
                error!("{:?}", e);
                let next = Error::<P>::new(e.to_string());
                return Transition::next(self, next);
            }
        }
        info!("Pod registered: {}", pod.name());
        let next = P::ImagePullState::default();
        Transition::next_unchecked(self, next)
    }

    async fn json_status(
        &self,
        _pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "Registered")
    }
}

impl<P: GenericProvider> TransitionTo<Error<P>> for Registered<P> {}
