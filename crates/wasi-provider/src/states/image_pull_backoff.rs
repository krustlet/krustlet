use super::image_pull::ImagePull;
use crate::PodState;
use kubelet::state::prelude::*;

state!(
    /// Kubelet encountered an error when pulling container image.
    ImagePullBackoff,
    PodState,
    {
        tokio::time::delay_for(std::time::Duration::from_secs(60)).await;
        Ok(Transition::Advance(Box::new(ImagePull)))
    },
    { make_status(Phase::Pending, "ImagePullBackoff") }
);
