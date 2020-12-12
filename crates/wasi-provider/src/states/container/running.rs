use super::terminated::Terminated;
use super::ContainerState;
use crate::ProviderState;
use kubelet::container::state::prelude::*;
use tokio::sync::mpsc::Receiver;

// while let Some((name, status)) = pod_state.run_context.status_recv.recv().await {
//     if let Err(e) = patch_container_status(
//         &client,
//         &pod,
//         &ContainerKey::Init(name.clone()),
//         &status,
//     )
//     .await
//     {
//         error!("Unable to patch status, will retry on next update: {:?}", e);
//     }

//     if let ContainerStatus::Terminated {
//         timestamp: _,
//         message,
//         failed,
//     } = status
//     {
//         if failed {
//             // HACK: update the status message informing which init container failed
//             let s = serde_json::json!({
//                 "metadata": {
//                     "resourceVersion": "",
//                 },
//                 "status": {
//                     "message": format!("Init container {} failed", name),
//                 }
//             });

//             // If we are in a failed state, insert in the init containers we already ran
//             // into a pod handle so they are available for future log fetching
//             let pod_handle = Handle::new(container_handles, pod.clone(), None);
//             let pod_key = PodKey::from(pod);
//             {
//                 let state_writer = provider_state.write().await;
//                 let mut handles = state_writer.handles.write().await;
//                 handles.insert(pod_key, Arc::new(pod_handle));
//             }

//             let status_json = match serde_json::to_vec(&s) {
//                 Ok(json) => json,
//                 Err(e) => fail_fatal!(e),
//             };

//             match client
//                 .patch_status(pod.name(), &PatchParams::default(), status_json)
//                 .await
//             {
//                 Ok(_) => return Transition::next(self, Error::<_>::new(message)),
//                 Err(e) => fail_fatal!(e),
//             };
//         } else {
//             break;
//         }
//     }
// }

/// The container is starting.
#[derive(Debug, TransitionTo)]
#[transition_to(Terminated)]
pub struct Running {
    rx: Receiver<(String, Status)>,
}

impl Running {
    pub fn new(rx: Receiver<(String, Status)>) -> Self {
        Running { rx }
    }
}

#[async_trait::async_trait]
impl State<ContainerState> for Running {
    async fn next(
        self: Box<Self>,
        _shared_state: SharedState<ProviderState>,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> Transition<ContainerState> {
        todo!()
    }

    async fn status(
        &self,
        _state: &mut ContainerState,
        _container: &Container,
    ) -> anyhow::Result<Status> {
        Ok(Status::running())
    }
}
