use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::TryFrom;
use std::ops::Deref;
use std::sync::Arc;

use log::{debug, error, info};
use tokio::sync::Mutex;

use kubelet::container::{Container, ContainerKey, Handle as ContainerHandle};
use kubelet::provider::Provider;
use kubelet::state::{State, Transition};
use kubelet::{
    pod::{key_from_pod, Handle, Phase, Pod},
    state,
};

use crate::rand::Rng;
use crate::VolumeBinding;
use crate::{make_status, PodState};
use crate::{wascc_run_http, ActorHandle, LogHandleFactory, WasccProvider};

use super::running::Running;

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
    port_map: &Arc<Mutex<BTreeMap<u16, String>>>,
    pod: &Pod,
) -> Result<u16, PortAllocationError> {
    let pod_key = key_from_pod(pod);
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
    port_map: Arc<Mutex<BTreeMap<u16, String>>>,
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
                    lock.insert(port_assigned, key_from_pod(pod));
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

async fn start_container(
    pod_state: &mut PodState,
    container: &Container,
    pod: &Pod,
    port_assigned: u16,
) -> anyhow::Result<ContainerHandle<ActorHandle, LogHandleFactory>> {
    let env =
        <WasccProvider as Provider>::env_vars(&container, &pod, &pod_state.shared.client).await;
    let volume_bindings: Vec<VolumeBinding> =
        if let Some(volume_mounts) = container.volume_mounts().as_ref() {
            volume_mounts
                .iter()
                .map(|vm| -> anyhow::Result<VolumeBinding> {
                    // Check the volume exists first
                    let vol = pod_state.run_context.volumes.get(&vm.name).ok_or_else(|| {
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
                .collect::<anyhow::Result<_>>()?
        } else {
            vec![]
        };

    debug!("Starting container {} on thread", container.name());

    let module_data = pod_state
        .run_context
        .modules
        .remove(container.name())
        .expect("FATAL ERROR: module map not properly populated");
    let lp = pod_state.shared.log_path.clone();
    let host = pod_state.shared.host.clone();
    tokio::task::spawn_blocking(move || {
        wascc_run_http(host, module_data, env, volume_bindings, &lp, port_assigned)
    })
    .await?
}

state!(
    /// The Kubelet is starting the Pod.
    Starting,
    PodState,
    {
        info!("Starting containers for pod {:?}", pod.name());

        let mut container_handles = HashMap::new();
        for container in pod.containers() {
            let port_assigned =
                assign_container_port(Arc::clone(&pod_state.shared.port_map), &pod, &container)
                    .await?;
            debug!(
                "New port assigned to {} is: {}",
                container.name(),
                port_assigned
            );

            let container_handle = start_container(pod_state, &container, &pod, port_assigned)
                .await
                .unwrap();
            container_handles.insert(
                ContainerKey::App(container.name().to_string()),
                container_handle,
            );
        }

        let pod_handle = Handle::new(container_handles, pod.clone(), None).await?;
        let pod_key = key_from_pod(&pod);
        {
            let mut handles = pod_state.shared.handles.write().await;
            handles.insert(pod_key, pod_handle);
        }

        info!("All containers started for pod {:?}.", pod.name());

        Ok(Transition::Advance(Box::new(Running)))
    },
    { make_status(Phase::Pending, "Starting") }
);
