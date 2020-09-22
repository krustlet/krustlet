use super::image_pull::ImagePull;
use crate::PodState;
use kubelet::backoff::BackoffStrategy;
use kubelet::state::prelude::*;

/// Kubelet encountered an error when pulling container image.
#[derive(Default, Debug)]
pub struct ImagePullBackoff;

#[async_trait::async_trait]
impl State<PodState> for ImagePullBackoff {
    async fn next(
        self: Box<Self>,
        pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        pod_state.image_pull_backoff_strategy.wait().await;
        Ok(Transition::next(self, ImagePull))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "ImagePullBackoff")
    }
}

impl TransitionTo<ImagePull> for ImagePullBackoff {}
