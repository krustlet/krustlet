use super::image_pull::ImagePull;
use crate::PodState;
use kubelet::backoff::BackoffStrategy;
use kubelet::pod::state::prelude::*;

/// Kubelet encountered an error when pulling container image.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(ImagePull)]
pub struct ImagePullBackoff;

#[async_trait::async_trait]
impl State<PodState, PodStatus> for ImagePullBackoff {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        pod_state.image_pull_backoff_strategy.wait().await;
        Transition::next(self, ImagePull)
    }

    async fn status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "ImagePullBackoff"))
    }
}
