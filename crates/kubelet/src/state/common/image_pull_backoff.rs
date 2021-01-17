//! Kubelet encountered an error when pulling container image.

use super::image_pull::ImagePull;
use super::{BackoffSequence, GenericPodState, GenericProvider};
use crate::pod::state::prelude::*;

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
impl<P: GenericProvider> State<P::PodState> for ImagePullBackoff<P> {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<P::ProviderState>,
        pod_state: &mut P::PodState,
        _pod: Manifest<Pod>,
    ) -> Transition<P::PodState> {
        pod_state.backoff(BackoffSequence::ImagePull).await;
        Transition::next(self, ImagePull::<P>::default())
    }

    async fn status(&self, _pod_state: &mut P::PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "ImagePullBackoff"))
    }
}

impl<P: GenericProvider> TransitionTo<ImagePull<P>> for ImagePullBackoff<P> {}
