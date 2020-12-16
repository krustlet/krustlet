use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::TryFrom;
use std::ops::Deref;
use std::sync::Arc;

use log::{debug, error, info};
use tokio::sync::Mutex;

use kubelet::container::state::prelude::*;
use kubelet::pod::{Handle as PodHandle, Pod, PodKey};
use kubelet::provider::Provider;

use crate::rand::Rng;
use crate::wascc_run;
use crate::ProviderState;
use crate::VolumeBinding;
use crate::WasccProvider;

use super::running::Running;
use super::terminated::Terminated;
use super::ContainerState;

#[derive(Debug)]
struct PortAllocationError;

impl std::fmt::Display for PortAllocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "all ports are currently in use")
    }
}

impl std::error::Error for PortAllocationError {
    fn description(&self) -> &str {
        "all ports are currently in use"
    }
}

async fn find_available_port(
    port_map: &Arc<Mutex<BTreeMap<u16, PodKey>>>,
    pod: &Pod,
) -> Result<u16, PortAllocationError> {
    let pod_key = PodKey::from(pod);
    let mut empty_port: BTreeSet<u16> = BTreeSet::new();
    let mut lock = port_map.lock().await;
    while empty_port.len() < 2768 {
        let generated_port: u16 = rand::thread_rng().gen_range(30000, 32768);
        if !lock.contains_key(&generated_port) {
            lock.insert(generated_port, pod_key);
            return Ok(generated_port);
        }
        empty_port.insert(generated_port);
    }
    Err(PortAllocationError)
}

async fn assign_container_port(
    port_map: Arc<Mutex<BTreeMap<u16, PodKey>>>,
    pod: &Pod,
    container: &Container,
) -> anyhow::Result<u16> {
    let mut port_assigned: u16 = 0;
    if let Some(container_vec) = container.ports().as_ref() {
        for c_port in container_vec.iter() {
            let container_port = c_port.container_port;
            if let Some(host_port) = c_port.host_port {
                let host_port: u16 = u16::try_from(host_port)?;
                let mut lock = port_map.lock().await;
                if !lock.contains_key(&host_port) {
                    port_assigned = host_port;
                    lock.insert(port_assigned, PodKey::from(pod));
                } else {
                    error!(
                        "Failed to assign hostport {}, because it's taken",
                        &host_port
                    );
                    return Err(anyhow::anyhow!("Port {} is currently in use", &host_port));
                }
            } else if container_port >= 0 && container_port <= 65536 {
                port_assigned = find_available_port(&port_map, pod).await?;
            }
        }
    }
    Ok(port_assigned)
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
        container: &Container,
    ) -> Transition<ContainerState> {
        info!(
            "Starting container {} for pod {}",
            container.name(),
            state.pod.name(),
        );

        let port_assigned = {
            let port_map = shared.read().await.port_map.clone();
            match assign_container_port(Arc::clone(&port_map), &state.pod, &container).await {
                Ok(port) => port,
                Err(e) => {
                    return Transition::next(
                        self,
                        Terminated::new(
                            format!(
                                "Pod {} container {} failed to allocate port: {:?}",
                                state.pod.name(),
                                container.name(),
                                e
                            ),
                            true,
                        ),
                    )
                }
            }
        };

        debug!(
            "New port assigned to {} is: {}",
            container.name(),
            port_assigned
        );

        let (client, log_path, host) = {
            let state_reader = shared.read().await;
            (
                state_reader.client.clone(),
                state_reader.log_path.clone(),
                state_reader.host.clone(),
            )
        };

        let env = <WasccProvider as Provider>::env_vars(&container, &state.pod, &client).await;
        let volume_bindings: Vec<VolumeBinding> =
            if let Some(volume_mounts) = container.volume_mounts().as_ref() {
                let run_context = state.run_context.read().await;
                match volume_mounts
                    .iter()
                    .map(|vm| -> anyhow::Result<VolumeBinding> {
                        // Check the volume exists first
                        let vol = run_context.volumes.get(&vm.name).ok_or_else(|| {
                            anyhow::anyhow!(
                                "no volume with the name of {} found for container {}",
                                vm.name,
                                container.name()
                            )
                        })?;
                        // We can safely assume that this should be valid UTF-8 because it would have
                        // been validated by the k8s API
                        Ok(VolumeBinding {
                            name: vm.name.clone(),
                            host_path: vol.deref().clone(),
                        })
                    })
                    .collect::<anyhow::Result<_>>()
                {
                    Ok(bindings) => bindings,
                    Err(e) => {
                        return Transition::next(
                            self,
                            Terminated::new(
                                format!(
                                    "Pod {} container {} failed to allocate storage: {:?}",
                                    state.pod.name(),
                                    container.name(),
                                    e
                                ),
                                true,
                            ),
                        )
                    }
                }
            } else {
                vec![]
            };

        debug!("Starting container {} on thread", container.name());

        let module_data = {
            let mut run_context = state.run_context.write().await;
            match run_context.modules.remove(container.name()) {
                Some(module) => module,
                None => {
                    return Transition::next(
                        self,
                        Terminated::new(
                            format!(
                                "FATAL ERROR: module map not properly populated ({}/{})",
                                state.pod.name(),
                                container.name(),
                            ),
                            true,
                        ),
                    )
                }
            }
        };

        match tokio::task::spawn_blocking(move || {
            wascc_run(
                host,
                module_data,
                env,
                volume_bindings,
                &log_path,
                port_assigned,
            )
        })
        .await
        {
            Ok(Ok(container_handle)) => {
                let pod_key = PodKey::from(&state.pod);
                {
                    let provider_state = shared.write().await;
                    let mut handles_writer = provider_state.handles.write().await;
                    let pod_handle = handles_writer
                        .entry(pod_key)
                        .or_insert_with(|| PodHandle::new(HashMap::new(), state.pod.clone(), None));
                    pod_handle
                        .insert_container_handle(state.container_key.clone(), container_handle)
                        .await;
                }
            }
            Ok(Err(e)) => {
                return Transition::next(
                    self,
                    Terminated::new(
                        format!(
                            "Pod {} container {} failed to start wascc actor: {:?}",
                            state.pod.name(),
                            container.name(),
                            e
                        ),
                        true,
                    ),
                )
            }
            Err(e) => {
                return Transition::next(
                    self,
                    Terminated::new(
                        format!(
                            "Pod {} container {} failed to start wascc actor: {:?}",
                            state.pod.name(),
                            container.name(),
                            e
                        ),
                        true,
                    ),
                )
            }
        }

        Transition::next(self, Running)
    }

    async fn status(
        &self,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> anyhow::Result<Status> {
        Ok(Status::waiting("Module is starting."))
    }
}
