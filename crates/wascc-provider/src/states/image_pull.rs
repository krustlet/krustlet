use kubelet::state::{State, Transition};
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
    {
        pod_state.run_context.modules = match pod_state.shared.store.fetch_pod_modules(&pod).await {
            Ok(modules) => modules,
            Err(e) => {
                error!("{:?}", e);
                return Ok(Transition::Error(Box::new(ImagePullBackoff)));
            }
        };
        Ok(Transition::Advance(Box::new(VolumeMount)))
    },
    { make_status(Phase::Pending, "ImagePull") }
);
