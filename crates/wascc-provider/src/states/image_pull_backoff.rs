use super::image_pull::ImagePull;
use crate::{make_status, PodState};
use kubelet::state::{PodChangeRx, State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};

state!(
    /// Kubelet encountered an error when pulling container image.
    ImagePullBackoff,
    PodState,
    ImagePull,
    ImagePullBackoff,
    { Ok(Transition::Advance(ImagePull)) },
    { make_status(Phase::Pending, "ImagePullBackoff") }
);
