//! Functions for running Pod state machines.
use crate::pod::{
    initialize_pod_container_statuses, make_status, status::patch_status, Phase, Pod, Status as PodStatus
};
use crate::state::{ResourceState, State, Transition, SharedState};
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::Api;
use log::{debug, error, warn};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Prelude for Pod state machines.
pub mod prelude {
    pub use crate::pod::{
        make_status, make_status_with_containers, Phase, Pod, Status as PodStatus,
    };
    pub use crate::state::{AsyncDrop, ResourceState, State, TransitionTo, SharedState, Transition};
    
}

/// Iteratively evaluate state machine until it returns Complete.
pub async fn run_to_completion<
    ProviderState: Send + Sync + 'static,
    PodState: Send + Sync + 'static + ResourceState<Manifest=Pod, Status=PodStatus>,
>(
    client: &kube::Client,
    state: impl State<ProviderState, PodState>,
    provider_state: SharedState<ProviderState>,
    pod_state: &mut PodState,
    pod: Arc<RwLock<Pod>>,
) {
    let (name, api) = {
        let initial_pod = pod.read().await.clone();
        let namespace = initial_pod.namespace().to_string();
        let name = initial_pod.name().to_string();
        let api: Api<KubePod> = Api::namespaced(client.clone(), &namespace);
        (name, api)
    };

    if initialize_pod_container_statuses(&name, Arc::clone(&pod), &api)
        .await
        .is_err()
    {
        return;
    }

    let mut state: Box<dyn State<ProviderState, PodState>> = Box::new(state);

    loop {
        debug!("Pod {} entering state {:?}", &name, state);

        let latest_pod = { pod.read().await.clone() };

        match state.status(pod_state, &latest_pod).await {
            Ok(patch) => {
                patch_status(&api, &name, patch).await;
            }
            Err(e) => {
                warn!("Pod {} status patch returned error: {:?}", &name, e);
            }
        }

        debug!("Pod {} executing state handler {:?}", &name, state);
        let transition = {
            state
                .next(provider_state.clone(), pod_state, &latest_pod)
                .await
        };

        state = match transition {
            Transition::Next(s) => {
                debug!("Pod {} transitioning to {:?}.", &name, s.state);
                s.state
            }
            Transition::Complete(result) => match result {
                Ok(()) => {
                    debug!("Pod {} state machine exited without error", &name);
                    break;
                }
                Err(e) => {
                    error!("Pod {} state machine exited with error: {:?}", &name, e);
                    let status = make_status(Phase::Failed, &format!("{:?}", e));
                    patch_status(&api, &name, status).await;
                    break;
                }
            },
        };
    }
}

#[derive(Default, Debug)]
/// Stub state machine for testing.
pub struct Stub;

#[async_trait::async_trait]
impl<Pr: 'static + Sync + Send, P: 'static + Sync + Send + ResourceState<Manifest=Pod, Status=PodStatus>> State<Pr, P> for Stub {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<Pr>,
        _pod_state: &mut P,
        _pod: &Pod,
    ) -> Transition<Pr, P> {
        Transition::Complete(Ok(()))
    }

    async fn status(&self, _state: &mut P, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(Default::default())
    }
}

#[cfg(test)]
mod test {
    use crate::pod::Pod;
    use crate::state::{SharedState, State, Transition, TransitionTo};

    #[derive(Debug)]
    struct ProviderState;

    #[derive(Debug)]
    struct PodState;

    #[derive(Debug)]
    struct ValidState;

    #[async_trait::async_trait]
    impl State<ProviderState, PodState> for ValidState {
        async fn next(
            self: Box<Self>,
            _provider_state: SharedState<ProviderState>,
            _pod_state: &mut PodState,
            _pod: &Pod,
        ) -> Transition<ProviderState, PodState> {
            Transition::Complete(Ok(()))
        }

        async fn status(&self, _state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
            Ok(Default::default())
        }
    }

    #[test]
    fn it_can_transition_to_valid_state() {
        #[derive(Debug)]
        struct TestState;

        impl TransitionTo<ValidState> for TestState {}

        #[async_trait::async_trait]
        impl State<ProviderState, PodState> for TestState {
            async fn next(
                self: Box<Self>,
                _provider_state: SharedState<ProviderState>,
                _pod_state: &mut PodState,
                _pod: &Pod,
            ) -> Transition<ProviderState, PodState> {
                Transition::next(self, ValidState)
            }

            async fn status(&self, _state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
                Ok(Default::default())
            }
        }
    }
}
