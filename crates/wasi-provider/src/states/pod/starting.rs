use std::collections::HashMap;
use std::sync::Arc;

use kubelet::pod::state::prelude::*;
use kubelet::pod::{Handle, PodKey};
use log::info;
use tokio::sync::Mutex;

use crate::states::container::ContainerHandleMap;
use crate::{PodState, ProviderState};

use super::running::Running;

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
impl State<PodState> for Starting {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        _pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<PodState> {
        let mut container_handles: ContainerHandleMap = HashMap::new();

        {
            let mut lock = self.init_handles.lock().await;
            container_handles.extend((*lock).drain())
        }

        info!("Starting containers for pod {:?}", pod.name());
        // for container in pod.containers() {
        //     match start_container(&provider_state, pod_state, &pod, &container).await {
        //         Ok(h) => {
        //             container_handles.insert(ContainerKey::App(container.name().to_string()), h);
        //         }
        //         // We should log, transition to running, and properly handle container failure.
        //         // Exiting here causes channel to be dropped messages to be lost from already running wasm runtimes.
        //         Err(e) => error!("Error spawning wasmtime: {:?}", e),
        //     }
        // }

        let pod_handle = Handle::new(container_handles, pod.clone(), None);
        let pod_key = PodKey::from(pod);
        {
            let state_reader = provider_state.read().await;
            let mut handles_writer = state_reader.handles.write().await;
            handles_writer.insert(pod_key, Arc::new(pod_handle));
        }
        info!("All containers started for pod {:?}.", pod.name());

        Transition::next(self, Running)
    }

    async fn status(&self, _pod_state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "Starting"))
    }
}
