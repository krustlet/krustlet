use kubelet::state::prelude::*;
use log::error;

use crate::PodState;

use super::image_pull_backoff::ImagePullBackoff;
use super::volume_mount::VolumeMount;

state!(
    /// Kubelet is pulling container images.
    ImagePull,
    PodState,
    {
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
                return Ok(Transition::Error(Box::new(ImagePullBackoff)));
            }
        };
        Ok(Transition::Advance(Box::new(VolumeMount)))
    },
    { make_status(Phase::Pending, "ImagePull") }
);
