use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use log::{debug, error, info};
use tokio::sync::Mutex;

use kubelet::container::{Container, ContainerKey};
use kubelet::pod::state::prelude::*;
use kubelet::pod::{Handle, PodKey};
use kubelet::provider;
use kubelet::state::common::GenericProviderState;
use kubelet::state::prelude::*;
use kubelet::volume::Ref;

use crate::wasi_runtime::{self, HandleFactory, Runtime, WasiRuntime};
use crate::{PodState, ProviderState};

use super::running::Running;
use crate::states::container::ContainerHandle;

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

pub(crate) async fn start_container(
    provider_state: &kubelet::state::prelude::SharedState<ProviderState>,
    pod_state: &mut PodState,
    pod: &Pod,
    container: &Container,
) -> anyhow::Result<ContainerHandle> {
    let (client, log_path) = {
        // Limit the time we hold the lock
        let state_reader = provider_state.read().await;
        (state_reader.client(), state_reader.log_path.clone())
    };

    let module_data = pod_state
        .run_context
        .modules
        .remove(container.name())
        .expect("FATAL ERROR: module map not properly populated");
    let env = provider::env_vars(&container, pod, &client).await;
    let args = container.args().clone().unwrap_or_default();
    let container_volumes = volume_path_map(container, &pod_state.run_context.volumes)?;

    let runtime = WasiRuntime::new(
        container.name().to_owned(),
        module_data,
        env,
        args,
        container_volumes,
        log_path,
        pod_state.run_context.status_sender.clone(),
    )
    .await?;

    debug!("Starting container {} on thread", container.name());
    runtime.start().await
}

pub(crate) type ContainerHandleMap =
    HashMap<ContainerKey, kubelet::container::Handle<Runtime, HandleFactory>>;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(Running)]
/// The Kubelet is starting the Pod containers
pub(crate) struct Starting {
    init_handles: Arc<Mutex<ContainerHandleMap>>,
}

impl Starting {
    pub(crate) fn new(init_handles: ContainerHandleMap) -> Self {
        Starting {
            init_handles: Arc::new(Mutex::new(init_handles)),
        }
    }
}

#[async_trait::async_trait]
impl State<ProviderState, PodState> for Starting {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<ProviderState, PodState> {
        let mut container_handles: ContainerHandleMap = HashMap::new();

        {
            let mut lock = self.init_handles.lock().await;
            container_handles.extend((*lock).drain())
        }

        info!("Starting containers for pod {:?}", pod.name());
        for container in pod.containers() {
            match start_container(&provider_state, pod_state, &pod, &container).await {
                Ok(h) => {
                    container_handles.insert(ContainerKey::App(container.name().to_string()), h);
                }
                // We should log, transition to running, and properly handle container failure.
                // Exiting here causes channel to be dropped messages to be lost from already running wasm runtimes.
                Err(e) => error!("Error spawning wasmtime: {:?}", e),
            }
        }

        let pod_handle = Arc::new(Handle::new(container_handles, pod.clone(), None));
        let pod_key = PodKey::from(pod);
        {
            let state_reader = provider_state.read().await;
            let mut handles_writer = state_reader.handles.write().await;
            handles_writer.insert(pod_key, pod_handle);
        }
        info!("All containers started for pod {:?}.", pod.name());

        Transition::next(self, Running)
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "Starting"))
    }
}
