use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use log::{debug, info};
use tokio::sync::Mutex;

use kubelet::container::{Container, ContainerKey};
use kubelet::pod::{key_from_pod, Handle};
use kubelet::provider;
use kubelet::state::prelude::*;
use kubelet::volume::Ref;

use crate::wasi_runtime::{self, HandleFactory, Runtime, WasiRuntime};
use crate::PodState;

use super::running::Running;

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
    pod_state: &mut PodState,
    pod: &Pod,
    container: &Container,
) -> anyhow::Result<kubelet::container::Handle<wasi_runtime::Runtime, wasi_runtime::HandleFactory>>
{
    let module_data = pod_state
        .run_context
        .modules
        .remove(container.name())
        .expect("FATAL ERROR: module map not properly populated");
    let client = kube::Client::new(pod_state.shared.kubeconfig.clone());
    let env = provider::env_vars(&container, pod, &client).await;
    let args = container.args().clone().unwrap_or_default();
    let container_volumes = volume_path_map(container, &pod_state.run_context.volumes)?;

    let runtime = WasiRuntime::new(
        container.name().to_owned(),
        module_data,
        env,
        args,
        container_volumes,
        pod_state.shared.log_path.clone(),
        pod_state.run_context.status_sender.clone(),
    )
    .await?;

    debug!("Starting container {} on thread", container.name());
    runtime.start().await
}

pub(crate) type ContainerHandleMap =
    HashMap<ContainerKey, kubelet::container::Handle<Runtime, HandleFactory>>;

#[derive(Default, Debug)]
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
impl State<PodState> for Starting {
    async fn next(
        self: Box<Self>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<Transition<PodState>> {
        let mut container_handles: ContainerHandleMap = HashMap::new();

        {
            let mut lock = self.init_handles.lock().await;
            container_handles.extend((*lock).drain())
        }

        info!("Starting containers for pod {:?}", pod.name());
        for container in pod.containers() {
            let container_handle = start_container(pod_state, &pod, &container).await?;
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

        Ok(Transition::next(self, Running))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "Starting")
    }
}

impl TransitionTo<Running> for Starting {}
