//! Kubelet is pulling container images.

use tracing::{error, info, instrument};

use super::{GenericPodState, GenericProvider, GenericProviderState};
use crate::pod::state::prelude::*;
use crate::provider::{PluginSupport, VolumeSupport};
use crate::state::common::error::Error;
use crate::volume::VolumeRef;

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

        let (client, volume_path, plugin_registry) = {
            let state_reader = provider_state.read().await;
            let vol_path = match state_reader.volume_path() {
                Some(p) => p.to_owned(),
                None => {
                    info!("No volume directory found for pod. Assuming no volume support");
                    return Transition::next_unchecked(self, P::RunState::default());
                }
            };
            (
                state_reader.client(),
                vol_path,
                state_reader.plugin_registry(),
            )
        };

        // Get the map of VolumeRefs
        let mut volumes = match VolumeRef::volumes_from_pod(&pod, &client, plugin_registry).await {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e);
                let next = Error::<P>::new(e.to_string());
                return Transition::next(self, next);
            }
        };
        // Now mount each volume
        let base_path = volume_path.join(pod_dir_name(&pod));
        let mounts = volumes
            .iter_mut()
            .map(|(k, v)| (k, v, base_path.clone()))
            .map(|(k, v, p)| async move {
                v.mount(p)
                    .await
                    .map_err(|e| anyhow::anyhow!("Unable to mount volume {}: {}", k, e))
            });
        if let Err(e) = futures::future::join_all(mounts)
            .await
            .into_iter()
            .collect::<anyhow::Result<()>>()
        {
            error!(error = %e);
            let next = Error::<P>::new(e.to_string());
            return Transition::next(self, next);
        }
        pod_state.set_volumes(volumes).await;
        Transition::next_unchecked(self, P::RunState::default())
    }

    async fn status(&self, _pod_state: &mut P::PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "VolumeMount"))
    }
}

impl<P: GenericProvider> TransitionTo<Error<P>> for VolumeMount<P> {}

fn pod_dir_name(pod: &Pod) -> String {
    format!("{}-{}", pod.name(), pod.namespace())
}
