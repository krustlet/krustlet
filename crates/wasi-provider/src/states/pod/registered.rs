use log::info;

use super::error::Error;
use super::image_pull::ImagePull;
use crate::transition_to_error;
use crate::PodState;
use kubelet::container::Container;
use kubelet::pod::state::prelude::*;

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

/// The Kubelet is aware of the Pod.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(ImagePull, Error)]
pub struct Registered;

#[async_trait::async_trait]
impl State<PodState, PodStatus> for Registered {
    async fn next(self: Box<Self>, _pod_state: &mut PodState, pod: &Pod) -> Transition<PodState> {
        match validate_pod_runnable(&pod) {
            Ok(_) => (),
            Err(e) => transition_to_error!(self, e),
        }
        info!("Pod added: {}.", pod.name());
        Transition::next(self, ImagePull)
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "Registered"))
    }
}
