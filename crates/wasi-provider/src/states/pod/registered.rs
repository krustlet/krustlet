use log::info;

use super::error::Error;
use super::image_pull::ImagePull;
use super::wont_run::WontRun;
use crate::transition_to_error;
use crate::PodState;
use kubelet::container::Container;
use kubelet::pod::state::prelude::*;

fn validate_pod_runnable(pod: &Pod) -> anyhow::Result<bool> {
    let is_kube_proxy = pod
        .containers()
        .iter()
        .map(validate_is_kube_proxy)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .any(|b| b);
    Ok(!is_kube_proxy)
}

fn validate_is_kube_proxy(container: &Container) -> anyhow::Result<bool> {
    if let Some(image) = container.image()? {
        if image.whole().starts_with("k8s.gcr.io/kube-proxy") {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The Kubelet is aware of the Pod.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(ImagePull, Error, WontRun)]
pub struct Registered;

#[async_trait::async_trait]
impl State<PodState> for Registered {
    async fn next(self: Box<Self>, _state: &mut PodState, pod: &Pod) -> Transition<PodState> {
        match validate_pod_runnable(&pod) {
            Ok(x) if x => {
                info!("Pod added: {}.", pod.name());
                Transition::next(self, ImagePull)
            }
            Ok(_) => {
                info!("Skipping non-wasi Pod: {}.", pod.name());
                Transition::next(self, WontRun)
            }
            Err(e) => transition_to_error!(self, e),
        }
    }

    async fn status(&self, _state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "Registered"))
    }
}
