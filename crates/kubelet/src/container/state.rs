//! Functions for running Container state machines.
use crate::container::{patch_container_status, Status};
use crate::container::{Container, ContainerKey};
use crate::pod::Pod;
use crate::state::{ResourceState, SharedState, State, Transition};
use chrono::Utc;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::Api;
use log::{debug, error, warn};
use tokio::sync::watch::{channel, Receiver};

/// Prelude for Pod state machines.
pub mod prelude {
    pub use crate::container::{Container, Handle, Status};
    pub use crate::state::{ResourceState, SharedState, State, Transition, TransitionTo};
    pub use tokio::sync::watch::Receiver;
}

/// Iteratively evaluate state machine until it returns Complete.
pub async fn run_to_completion<S: ResourceState<Manifest = Container, Status = Status>>(
    client: &kube::Client,
    initial_state: impl State<S>,
    shared: SharedState<S::SharedState>,
    mut container_state: S,
    mut pod: Receiver<Pod>,
    container_name: ContainerKey,
) -> anyhow::Result<()> {
    let initial_pod = pod
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("Manifest sender dropped."))?;
    let namespace = initial_pod.namespace().to_string();
    let pod_name = initial_pod.name().to_string();
    let api: Api<KubePod> = Api::namespaced(client.clone(), &namespace);

    let mut state: Box<dyn State<S>> = Box::new(initial_state);

    // Forward pod updates as container updates.
    let initial_container = initial_pod.find_container(&container_name).unwrap();
    let (container_tx, container_rx) = channel(initial_container);
    let mut task_pod = pod.clone();
    let task_container_name = container_name.clone();
    tokio::spawn(async move {
        while let Some(latest_pod) = task_pod.recv().await {
            let latest_container = latest_pod.find_container(&task_container_name).unwrap();
            match container_tx.broadcast(latest_container) {
                Ok(()) => (),
                Err(e) => {
                    warn!("Unable to broadcast container update: {:?}", e);
                    return;
                }
            }
        }
    });

    loop {
        debug!(
            "Pod {} container {} entering state {:?}",
            &pod_name, container_name, state
        );

        let latest_pod = pod
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Manifest sender dropped."))?;
        let latest_container = latest_pod.find_container(&container_name).unwrap();

        match state.status(&mut container_state, &latest_container).await {
            Ok(status) => {
                match patch_container_status(&api, &latest_pod, &container_name, &status).await {
                    Ok(_) => (),
                    Err(e) => {
                        warn!(
                            "Pod {} container {} status patch request returned error: {:?}",
                            &pod_name, container_name, e
                        );
                    }
                }
            }
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
        let transition = {
            state
                .next(shared.clone(), &mut container_state, container_rx.clone())
                .await
        };

        state = match transition {
            Transition::Next(s) => {
                let state = s.into();
                debug!(
                    "Pod {} container {} transitioning to {:?}.",
                    &pod_name, container_name, state
                );
                state
            }
            Transition::Complete(result) => match result {
                Ok(()) => {
                    debug!(
                        "Pod {} container {} state machine exited without error",
                        &pod_name, container_name
                    );
                    break result;
                }
                Err(ref e) => {
                    error!(
                        "Pod {} container {} state machine exited with error: {:?}",
                        &pod_name, container_name, e
                    );
                    let status = Status::Terminated {
                        timestamp: Utc::now(),
                        message: format!("Container exited with error: {:?}.", e),
                        failed: true,
                    };
                    patch_container_status(&api, &latest_pod, &container_name, &status)
                        .await
                        .unwrap();

                    break result;
                }
            },
        };
    }
}
