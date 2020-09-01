use log::{error, info};

use crate::{make_status, PodState};
use kubelet::state::{State, Transition};
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
    {
        info!("Pod added: {}.", pod.name());
        match validate_pod_runnable(&pod) {
            Ok(_) => (),
            Err(e) => {
                let message = format!("{:?}", e);
                error!("{}", message);
                return Ok(Transition::Error(Box::new(Error { message })));
            }
        }
        info!("Pod validated: {}.", pod.name());
        info!("Pod registered: {}.", pod.name());
        Ok(Transition::Advance(Box::new(ImagePull)))
    },
    { make_status(Phase::Pending, "Registered") }
);

#[cfg(test)]
mod test {
    use super::*;
    use k8s_openapi::api::core::v1::Container as KubeContainer;
    use k8s_openapi::api::core::v1::Pod as KubePod;
    use serde_json::json;

    fn make_pod_spec(containers: Vec<KubeContainer>) -> Pod {
        let kube_pod: KubePod = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": "test-pod-spec"
            },
            "spec": {
                "containers": containers
            }
        }))
        .unwrap();
        Pod::new(kube_pod)
    }

    #[test]
    fn can_run_pod_where_container_has_no_args() {
        let containers: Vec<KubeContainer> = serde_json::from_value(json!([
            {
                "name": "greet-wascc",
                "image": "webassembly.azurecr.io/greet-wascc:v0.4",
            },
        ]))
        .unwrap();
        let pod = make_pod_spec(containers);
        validate_pod_runnable(&pod).unwrap();
    }

    #[test]
    fn can_run_pod_where_container_has_empty_args() {
        let containers: Vec<KubeContainer> = serde_json::from_value(json!([
            {
                "name": "greet-wascc",
                "image": "webassembly.azurecr.io/greet-wascc:v0.4",
                "args": [],
            },
        ]))
        .unwrap();
        let pod = make_pod_spec(containers);
        validate_pod_runnable(&pod).unwrap();
    }

    #[test]
    fn cannot_run_pod_where_container_has_args() {
        let containers: Vec<KubeContainer> = serde_json::from_value(json!([
            {
                "name": "greet-wascc",
                "image": "webassembly.azurecr.io/greet-wascc:v0.4",
                "args": [
                    "--foo",
                    "--bar"
                ]
            },
        ]))
        .unwrap();
        let pod = make_pod_spec(containers);
        assert!(validate_pod_runnable(&pod).is_err());
    }

    #[test]
    fn cannot_run_pod_where_any_container_has_args() {
        let containers: Vec<KubeContainer> = serde_json::from_value(json!([
            {
                "name": "greet-1",
                "image": "webassembly.azurecr.io/greet-wascc:v0.4"
            },
            {
                "name": "greet-2",
                "image": "webassembly.azurecr.io/greet-wascc:v0.4",
                "args": [
                    "--foo",
                    "--bar"
                ]
            },
        ]))
        .unwrap();
        let pod = make_pod_spec(containers);
        let validation = validate_pod_runnable(&pod);
        assert!(validation.is_err());
        let message = format!("{}", validation.unwrap_err());
        assert!(
            message.contains("greet-2"),
            "validation error did not give name of bad container"
        );
    }
}
