use crate::PodState;
use chrono::Utc;
use k8s_openapi::api::core::v1::ContainerState as KubeContainerState;
use k8s_openapi::api::core::v1::ContainerStateRunning as KubeContainerStateRunning;
use k8s_openapi::api::core::v1::ContainerStatus as KubeContainerStatus;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time as KubeTime;
use kubelet::state::prelude::*;

/// The Kubelet is running the Pod.
#[derive(Default, Debug)]
pub struct Running;

#[async_trait::async_trait]
impl State<PodState> for Running {
    async fn next(self: Box<Self>, _pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        // Wascc has no notion of exiting so we just sleep.
        // I _think_ that periodically awaiting will allow the task to be interrupted.
        loop {
            tokio::time::delay_for(std::time::Duration::from_secs(10)).await;
        }
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        let ts = Utc::now();
        let container_statuses: Vec<KubeContainerStatus> = pod
            .containers()
            .iter()
            .map(|container| {
                // Create empty patch and update only the fields we want to change.
                let mut status: KubeContainerStatus = Default::default();
                let mut state: KubeContainerState = Default::default();
                status.name = container.name().to_string();
                status.ready = true;
                status.started = Some(true);
                state.running = Some(KubeContainerStateRunning {
                    started_at: Some(KubeTime(ts)),
                });
                status.state = Some(state);
                status
            })
            .collect();
        make_status_with_containers(Phase::Running, "Running", container_statuses)
    }
}
