//! Kubelet encountered an error when pulling container image.

use crate::state::prelude::*;

use super::image_pull::ImagePull;
use super::{BackoffSequence, GenericPodState, GenericProvider};

/// Kubelet encountered an error when pulling container image.
pub struct ImagePullBackoff<P: GenericProvider> {
    phantom: std::marker::PhantomData<P>,
}

impl<P: GenericProvider> std::fmt::Debug for ImagePullBackoff<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "ImagePullBackoff".fmt(formatter)
    }
}

impl<P: GenericProvider> Default for ImagePullBackoff<P> {
    fn default() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<P: GenericProvider> State<P::ProviderState, P::PodState> for ImagePullBackoff<P> {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<P::ProviderState>,
        pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> Transition<P::ProviderState, P::PodState> {
        pod_state.backoff(BackoffSequence::ImagePull).await;
        Transition::next(self, ImagePull::<P>::default())
    }

    async fn json_status(
        &self,
        _pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "ImagePullBackoff")
    }
}

impl<P: GenericProvider> TransitionTo<ImagePull<P>> for ImagePullBackoff<P> {}
