use kubelet::backoff::BackoffStrategy;
use kubelet::state::prelude::*;
use log::error;

use crate::PodState;

use super::image_pull_backoff::ImagePullBackoff;
use super::volume_mount::VolumeMount;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(VolumeMount, ImagePullBackoff)]
pub struct ImagePull;

#[async_trait::async_trait]
impl State<PodState> for ImagePull {
    async fn next(self: Box<Self>, pod_state: &mut PodState, pod: &Pod) -> Transition<PodState> {
        let client = kube::Client::new(pod_state.shared.kubeconfig.clone());
        let auth_resolver = kubelet::secret::RegistryAuthResolver::new(client, &pod);
        pod_state.run_context.modules = match pod_state
            .shared
            .store
            .fetch_pod_modules(&pod, &auth_resolver)
            .await
        {
            Ok(modules) => modules,
            Err(e) => {
                error!("{:?}", e);
                return Transition::next(self, ImagePullBackoff);
            }
        };
        pod_state.image_pull_backoff_strategy.reset();
        Transition::next(self, VolumeMount)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "ImagePull")
    }
}
