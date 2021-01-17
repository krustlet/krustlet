use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use log::{debug, info};
use tokio::sync::mpsc;

use kubelet::container::state::prelude::*;
use kubelet::pod::{Handle as PodHandle, PodKey};
use kubelet::state::common::GenericProviderState;
use kubelet::volume::Ref;

use crate::wasi_runtime::WasiRuntime;
use crate::ProviderState;

use super::running::Running;
use super::terminated::Terminated;
use super::ContainerState;

fn volume_path_map(
    container: &Container,
    volumes: &HashMap<String, Ref>,
) -> anyhow::Result<HashMap<PathBuf, Option<PathBuf>>> {
    if let Some(volume_mounts) = container.volume_mounts().as_ref() {
        volume_mounts
            .iter()
            .map(|vm| -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
                // Check the volume exists first
                let vol = volumes.get(&vm.name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "no volume with the name of {} found for container {}",
                        vm.name,
                        container.name()
                    )
                })?;
                let mut guest_path = PathBuf::from(&vm.mount_path);
                if let Some(sub_path) = &vm.sub_path {
                    guest_path.push(sub_path);
                }
                // We can safely assume that this should be valid UTF-8 because it would have
                // been validated by the k8s API
                Ok((vol.deref().clone(), Some(guest_path)))
            })
            .collect::<anyhow::Result<HashMap<PathBuf, Option<PathBuf>>>>()
    } else {
        Ok(HashMap::default())
    }
}

/// The container is starting.
#[derive(Default, Debug, TransitionTo)]
#[transition_to(Running, Terminated)]
pub struct Waiting;

#[async_trait::async_trait]
impl State<ContainerState> for Waiting {
    async fn next(
        self: Box<Self>,
        shared: SharedState<ProviderState>,
        state: &mut ContainerState,
        container: Manifest<Container>,
    ) -> Transition<ContainerState> {
        let container = container.latest();

        info!(
            "Starting container {} for pod {}",
            container.name(),
            state.pod.name(),
        );

        let (client, log_path) = {
            let provider_state = shared.read().await;
            (provider_state.client(), provider_state.log_path.clone())
        };

        let (module_data, container_volumes) = {
            let mut run_context = state.run_context.write().await;
            let module_data = match run_context.modules.remove(container.name()) {
                Some(data) => data,
                None => {
                    return Transition::next(
                        self,
                        Terminated::new(
                            format!(
                                "Pod {} container {} failed load module data from run context.",
                                state.pod.name(),
                                container.name(),
                            ),
                            true,
                        ),
                    );
                }
            };
            let container_volumes = match volume_path_map(&container, &run_context.volumes) {
                Ok(volumes) => volumes,
                Err(e) => {
                    return Transition::next(
                        self,
                        Terminated::new(
                            format!(
                                "Pod {} container {} failed to map volume paths: {:?}",
                                state.pod.name(),
                                container.name(),
                                e
                            ),
                            true,
                        ),
                    )
                }
            };
            (module_data, container_volumes)
        };

        let env = kubelet::provider::env_vars(&container, &state.pod, &client).await;
        let args = container.args().clone().unwrap_or_default();

        // TODO: ~magic~ number
        let (tx, rx) = mpsc::channel(8);

        let runtime = match WasiRuntime::new(
            container.name().to_owned(),
            module_data,
            env,
            args,
            container_volumes,
            log_path,
            tx,
        )
        .await
        {
            Ok(runtime) => runtime,
            Err(e) => {
                return Transition::next(
                    self,
                    Terminated::new(
                        format!(
                            "Pod {} container {} failed to construct runtime: {:?}",
                            state.pod.name(),
                            container.name(),
                            e
                        ),
                        true,
                    ),
                )
            }
        };
        debug!("Starting container {} on thread", container.name());
        let container_handle = match runtime.start().await {
            Ok(handle) => handle,
            Err(e) => {
                return Transition::next(
                    self,
                    Terminated::new(
                        format!(
                            "Pod {} container {} failed to start: {:?}",
                            state.pod.name(),
                            container.name(),
                            e
                        ),
                        true,
                    ),
                )
            }
        };
        let pod_key = PodKey::from(&state.pod);
        {
            let provider_state = shared.write().await;
            let mut handles_writer = provider_state.handles.write().await;
            let pod_handle = handles_writer.entry(pod_key).or_insert_with(|| {
                Arc::new(PodHandle::new(HashMap::new(), state.pod.clone(), None))
            });
            pod_handle
                .insert_container_handle(state.container_key.clone(), container_handle)
                .await;
        }
        Transition::next(self, Running::new(rx))
    }

    async fn status(
        &self,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> anyhow::Result<Status> {
        Ok(Status::waiting("Module is starting."))
    }
}
