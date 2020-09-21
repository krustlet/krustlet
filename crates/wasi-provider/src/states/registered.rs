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

/// The Kubelet is aware of the Pod.
#[derive(Default, Debug)]
pub struct Registered;

#[async_trait::async_trait]
impl State<PodState> for Registered {
    async fn next(
        self: Box<Self>,
        _pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        match validate_pod_runnable(&pod) {
            Ok(_) => (),
            Err(e) => {
                let message = format!("{:?}", e);
                error!("{}", message);
                return Ok(Transition::next(self, Error { message }));
            }
        }
        info!("Pod added: {}.", pod.name());
        Ok(Transition::next(self, ImagePull))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "Registered")
    }
}

impl TransitionTo<ImagePull> for Registered {}
impl TransitionTo<Error> for Registered {}
