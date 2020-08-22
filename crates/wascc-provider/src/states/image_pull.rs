use kubelet::state::{PodChangeRx, State, Transition};
use kubelet::{
    pod::{Phase, Pod},
    state,
};
use log::error;

use crate::{make_status, PodState};

use super::image_pull_backoff::ImagePullBackoff;
use super::volume_mount::VolumeMount;

state!(
    /// Kubelet is pulling container images.
    ImagePull,
    PodState,
    VolumeMount,
    ImagePullBackoff,
    {
        pod_state.run_context.modules = match pod_state.store.fetch_pod_modules(&pod).await {
            Ok(modules) => modules,
            Err(e) => {
                error!("{:?}", e);
                return Ok(Transition::Error(ImagePullBackoff));
            }
        };
        Ok(Transition::Advance(VolumeMount))
    },
    { make_status(Phase::Pending, "ImagePull") }
);
