//! Functions for running Container state machines.
use crate::container::{patch_container_status, Status};
use crate::container::{Container, ContainerKey};
use crate::pod::Pod;
use crate::state::{ResourceState, State, Transition};
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::Api;
use log::{debug, error, warn};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Iteratively evaluate state machine until it returns Complete.
pub async fn run_to_completion<S: ResourceState<Manifest = Container> + Send + Sync + 'static>(
    client: &kube::Client,
    initial_state: impl State<S, Status>,
    container_state: &mut S,
    pod: Arc<RwLock<Pod>>,
    container_name: ContainerKey,
) {
    let (pod_name, api) = {
        let initial_pod = pod.read().await.clone();
        let namespace = initial_pod.namespace().to_string();
        let name = initial_pod.name().to_string();
        let api: Api<KubePod> = Api::namespaced(client.clone(), &namespace);
        (name, api)
    };

    let mut state: Box<dyn State<S, Status>> = Box::new(initial_state);

    loop {
        debug!(
            "Pod {} container {} entering state {:?}",
            &pod_name, container_name, state
        );

        let latest_pod = { pod.read().await.clone() };
        let latest_container = latest_pod.find_container(&container_name).unwrap();

        // TODO Make json_status -> status
        match state.json_status(container_state, &latest_container).await {
            Ok(status) => patch_container_status(&api, &latest_pod, &container_name, &status)
                .await
                .unwrap(),
            Err(e) => {
                warn!(
                    "Pod {} container {} status patch returned error: {:?}",
                    &pod_name, container_name, e
                );
            }
        }

        debug!(
            "Pod {} container {} executing state handler {:?}",
            &pod_name, container_name, state
        );
        let transition = { state.next(container_state, &latest_container).await };

        state = match transition {
            Transition::Next(s) => {
                debug!(
                    "Pod {} container {} transitioning to {:?}.",
                    &pod_name, container_name, s.state
                );
                s.state
            }
            Transition::Complete(result) => match result {
                Ok(()) => {
                    debug!(
                        "Pod {} container {} state machine exited without error",
                        &pod_name, container_name
                    );
                    // TODO Patch container success.
                    break;
                }
                Err(e) => {
                    error!(
                        "Pod {} container {} state machine exited with error: {:?}",
                        &pod_name, container_name, e
                    );
                    // TODO Patch container failure.
                    break;
                }
            },
        };
    }
}
