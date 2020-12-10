//! The Kubelet is aware of the Pod.

use log::{debug, error, info};
use crate::pod::state::prelude::*;

use super::error::Error;
use super::image_pull::ImagePull;
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
        let next = ImagePull::<P>::default();
        Transition::next(self, next)
    }

    async fn status(
        &self,
        _pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> anyhow::Result<<P::PodState as ResourceState>::Status> {
        Ok(make_status(Phase::Pending, "Registered"))
    }
}

impl<P: GenericProvider> TransitionTo<Error<P>> for Registered<P> {}
impl<P: GenericProvider> TransitionTo<ImagePull<P>> for Registered<P> {}
