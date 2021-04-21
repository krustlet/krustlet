//! Kubelet is pulling container images.

use super::image_pull_backoff::ImagePullBackoff;
use super::volume_mount::VolumeMount;
use super::{BackoffSequence, GenericPodState, GenericProvider, GenericProviderState};
use crate::pod::state::prelude::*;

use tracing::{error, instrument};

/// Kubelet is pulling container images.
pub struct ImagePull<P: GenericProvider> {
    phantom: std::marker::PhantomData<P>,
}

impl<P: GenericProvider> std::fmt::Debug for ImagePull<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "ImagePull".fmt(formatter)
    }
}

impl<P: GenericProvider> Default for ImagePull<P> {
    fn default() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<P: GenericProvider> State<P::PodState> for ImagePull<P> {
    #[instrument(
        level = "info",
        skip(self, provider_state, pod_state, pod),
        fields(pod_name)
    )]
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<P::ProviderState>,
        pod_state: &mut P::PodState,
        pod: Manifest<Pod>,
    ) -> Transition<P::PodState> {
        let pod = pod.latest();

        tracing::Span::current().record("pod_name", &pod.name());

        let (client, store) = {
            // Minimise the amount of time we hold any locks
            let state_reader = provider_state.read().await;
            (state_reader.client(), state_reader.store())
        };
        let auth_resolver = crate::secret::RegistryAuthResolver::new(client, &pod);
        let modules = match store.fetch_pod_modules(&pod, &auth_resolver).await {
            Ok(m) => m,
            Err(e) => {
                error!(error = %e);
                return Transition::next(self, ImagePullBackoff::<P>::default());
            }
        };
        pod_state.set_modules(modules).await;
        pod_state.reset_backoff(BackoffSequence::ImagePull).await;
        Transition::next(self, VolumeMount::<P>::default())
    }

    async fn status(&self, _pod_state: &mut P::PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "ImagePull"))
    }
}

impl<P: GenericProvider> TransitionTo<ImagePullBackoff<P>> for ImagePull<P> {}
impl<P: GenericProvider> TransitionTo<VolumeMount<P>> for ImagePull<P> {}
