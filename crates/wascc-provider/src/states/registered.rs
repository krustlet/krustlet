use log::{error, info};

use crate::{make_status, PodState};
use kubelet::state::{PodChangeRx, State, Transition};
use kubelet::{
    container::Container,
    pod::{Phase, Pod},
    state,
};

use super::error::Error;
use super::image_pull::ImagePull;

fn validate_pod_runnable(pod: &Pod) -> anyhow::Result<()> {
    if !pod.init_containers().is_empty() {
        return Err(anyhow::anyhow!(
            "Cannot run {}: spec specifies init containers which are not supported on wasCC",
            pod.name()
        ));
    }
    for container in pod.containers() {
        validate_container_runnable(&container)?;
    }
    Ok(())
}

fn validate_container_runnable(container: &Container) -> anyhow::Result<()> {
    if has_args(container) {
        return Err(anyhow::anyhow!(
            "Cannot run {}: spec specifies container args which are not supported on wasCC",
            container.name()
        ));
    }

    Ok(())
}

fn has_args(container: &Container) -> bool {
    match &container.args() {
        None => false,
        Some(vec) => !vec.is_empty(),
    }
}

state!(
    /// The Kubelet is aware of the Pod.
    Registered,
    PodState,
    ImagePull,
    Error,
    {
        info!("Pod added: {}.", pod.name());
        match validate_pod_runnable(&pod) {
            Ok(_) => (),
            Err(e) => {
                let message = format!("{:?}", e);
                error!("{}", message);
                return Ok(Transition::Error(Error { message }));
            }
        }
        info!("Pod validated: {}.", pod.name());
        info!("Pod registered: {}.", pod.name());
        Ok(Transition::Advance(ImagePull))
    },
    { make_status(Phase::Pending, "Registered") }
);
