use log::{error, info};

use super::error::Error;
use super::image_pull::ImagePull;
use crate::PodState;
use kubelet::container::Container;
use kubelet::state::prelude::*;

fn validate_pod_runnable(pod: &Pod) -> anyhow::Result<()> {
    for container in pod.containers() {
        validate_not_kube_proxy(&container)?;
    }
    Ok(())
}

fn validate_not_kube_proxy(container: &Container) -> anyhow::Result<()> {
    if let Some(image) = container.image()? {
        if image.whole().starts_with("k8s.gcr.io/kube-proxy") {
            return Err(anyhow::anyhow!("Cannot run kube-proxy"));
        }
    }
    Ok(())
}

state!(
    /// The Kubelet is aware of the Pod.
    Registered,
    PodState,
    {
        match validate_pod_runnable(&pod) {
            Ok(_) => (),
            Err(e) => {
                let message = format!("{:?}", e);
                error!("{}", message);
                return Ok(Transition::Error(Box::new(Error { message })));
            }
        }
        info!("Pod added: {}.", pod.name());
        Ok(Transition::Advance(Box::new(ImagePull)))
    },
    { make_status(Phase::Pending, "Registered") }
);
