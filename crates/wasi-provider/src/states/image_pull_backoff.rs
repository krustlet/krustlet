use super::image_pull::ImagePull;
use crate::PodState;
use kubelet::state::prelude::*;

/// Kubelet encountered an error when pulling container image.
#[derive(Default, Debug)]
pub struct ImagePullBackoff;

#[async_trait::async_trait]
impl State<PodState> for ImagePullBackoff {
    async fn next(
        self: Box<Self>,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        tokio::time::delay_for(std::time::Duration::from_secs(60)).await;
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
