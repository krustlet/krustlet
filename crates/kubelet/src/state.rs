//! Used to define a state machine of Pod states.
use log::{debug, error, warn};

pub mod prelude;

use crate::pod::make_registered_status;
use crate::pod::{Phase, Pod};
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(feature = "derive")]
#[doc(hidden)]
pub use kubelet_derive::*;

/// Holds arbitrary State objects in Box, and prevents manual construction of Transition::Next
///
/// ```compile_fail
/// use kubelet::state::{Transition, StateHolder, Stub};
///
/// struct PodState;
///
/// // This fails because `state` is a private field. Use Transition::next classmethod instead.
/// let _transition = Transition::<PodState>::Next(StateHolder {
///     state: Box::new(Stub),
/// });
/// ```
pub struct StateHolder<PodState> {
    // This is private, preventing manual construction of Transition::Next
    state: Box<dyn State<PodState>>,
}

/// Represents result of state execution and which state to transition to next.
pub enum Transition<PodState> {
    /// Transition to new state.
    Next(StateHolder<PodState>),
    /// Stop executing the state machine and report the result of the execution.
    Complete(anyhow::Result<()>),
}

/// Mark an edge exists between two states.
pub trait TransitionTo<S> {}

impl<PodState> Transition<PodState> {
    // This prevents user from having to box everything AND allows us to enforce edge constraint.
    /// Construct Transition::Next from old state and new state. Both states must be State<PodState>
    /// with matching PodState. Input state must implement TransitionTo<OutputState>, which can be
    /// done manually or with the `TransitionTo` derive macro (requires the `derive` feature to be
    /// enabled)
    ///
    /// ```
    /// use kubelet::state::{Transition, State, TransitionTo};
    /// use kubelet::pod::Pod;
    ///
    /// #[derive(Debug, TransitionTo)]
    /// #[transition_to(TestState)]
    /// struct TestState;
    ///
    /// // Example of manual trait implementation
    /// // impl TransitionTo<TestState> for TestState {}
    ///
    /// struct PodState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> Transition<PodState> {
    ///         Transition::next(self, TestState)
    ///     }
    ///
    ///     async fn json_status(
    ///         &self,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<serde_json::Value> {
    ///         Ok(serde_json::json!(null))
    ///     }
    /// }
    /// ```
    ///
    /// The next state must also be State<PodState>, if it is not State, it fails to compile:
    /// ```compile_fail
    /// use kubelet::state::{Transition, State, TransitionTo};
    /// use kubelet::pod::Pod;
    ///
    /// #[derive(Debug, TransitionTo)]
    /// #[transition_to(NotState)]
    /// struct TestState;
    ///
    /// struct PodState;
    ///
    /// #[derive(Debug)]
    /// struct NotState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<Transition<PodState>> {
    ///         // This fails because NotState is not State
    ///         Ok(Transition::next(self, NotState))
    ///     }
    ///
    ///     async fn json_status(
    ///         &self,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<serde_json::Value> {
    ///         Ok(serde_json::json!(null))
    ///     }
    /// }
    /// ```
    ///
    /// Edges must be defined, even for self-transition, with edge removed, compilation fails:
    /// ```compile_fail
    /// use kubelet::state::{Transition, State};
    /// use kubelet::pod::Pod;
    ///
    /// #[derive(Debug)]
    /// struct TestState;
    ///
    /// // impl TransitionTo<TestState> for TestState {}
    ///
    /// struct PodState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<Transition<PodState>> {
    ///         // This fails because TestState is not TransitionTo<TestState>
    ///         Ok(Transition::next(self, TestState))
    ///     }
    ///
    ///     async fn json_status(
    ///         &self,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<serde_json::Value> {
    ///         Ok(serde_json::json!(null))
    ///     }
    /// }
    /// ```
    ///
    /// The next state must have the same PodState type, otherwise compilation will fail:
    /// ```compile_fail
    /// use kubelet::state::{Transition, State, TransitionTo};
    /// use kubelet::pod::Pod;
    ///
    /// #[derive(Debug, TransitionTo)]
    /// #[transition_to(OtherState)]
    /// struct TestState;
    ///
    /// struct PodState;
    ///
    /// #[derive(Debug)]
    /// struct OtherState;
    ///
    /// struct OtherPodState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<Transition<PodState>> {
    ///         // This fails because OtherState is State<OtherPodState>
    ///         Ok(Transition::next(self, OtherState))
    ///     }
    ///
    ///     async fn json_status(
    ///         &self,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<serde_json::Value> {
    ///         Ok(serde_json::json!(null))
    ///     }
    /// }
    ///
    /// #[async_trait::async_trait]
    /// impl State<OtherPodState> for OtherState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _pod_state: &mut OtherPodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<Transition<OtherPodState>> {
    ///         Ok(Transition::Complete(Ok(())))
    ///     }
    ///
    ///     async fn json_status(
    ///         &self,
    ///         _pod_state: &mut OtherPodState,
    ///         _pod: &Pod,
    ///     ) -> anyhow::Result<serde_json::Value> {
    ///         Ok(serde_json::json!(null))
    ///     }
    /// }
    /// ```
    #[allow(clippy::boxed_local)]
    pub fn next<I: State<PodState>, S: State<PodState>>(_i: Box<I>, s: S) -> Transition<PodState>
    where
        I: TransitionTo<S>,
    {
        Transition::Next(StateHolder { state: Box::new(s) })
    }
}

#[async_trait::async_trait]
/// Allow for asynchronous cleanup up of PodState.
pub trait AsyncDrop: Sized {
    /// Clean up PodState.
    async fn async_drop(self);
}

#[async_trait::async_trait]
/// A trait representing a node in the state graph.
pub trait State<PodState>: Sync + Send + 'static + std::fmt::Debug {
    /// Provider supplies method to be executed when in this state.
    async fn next(self: Box<Self>, pod_state: &mut PodState, pod: &Pod) -> Transition<PodState>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn json_status(
        &self,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<serde_json::Value>;
}

async fn patch_status(api: &Api<KubePod>, name: &str, patch: serde_json::Value) {
    match serde_json::to_vec(&patch) {
        Ok(data) => {
            debug!(
                "Applying status patch to Pod {}: '{}'",
                &name,
                std::str::from_utf8(&data).unwrap()
            );
            match api.patch_status(&name, &PatchParams::default(), data).await {
                Ok(_) => (),
                Err(e) => {
                    warn!("Pod {} error patching status: {:?}", name, e);
                }
            }
        }
        Err(e) => {
            warn!(
                "Pod {} error serializing status patch {:?}: {:?}",
                name, &patch, e
            );
        }
    }
}

/// Iteratively evaluate state machine until it returns Complete.
pub async fn run_to_completion<PodState: Send + Sync + 'static>(
    client: &kube::Client,
    state: impl State<PodState>,
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

    // NOTE: This loop patches the container statuses of the Pod with and then
    // waits for them to be picked up by the reflector. This is needed for a
    // few reasons:
    // * Kubernetes rewrites an empty array to null, preventing us from
    //   starting with that and appending.
    // * Pod reflection is not updated within a given state, meaning that
    //   container status patching cannot be responsible for initializing this
    //   (this would be a race condition anyway).
    // I'm not sure if we want to loop forever or handle some sort of failure
    // condition (if Kubernetes refuses to accept and propagate this
    // initialization patch.)
    'main: loop {
        let (num_containers, num_init_containers) = {
            let pod = pod.read().await;
            patch_status(&api, &name, make_registered_status(&pod)).await;
            let num_containers = pod.containers().len();
            let num_init_containers = pod.init_containers().len();
            (num_containers, num_init_containers)
        };
        for _ in 0..10 {
            let status = {
                pod.read()
                    .await
                    .as_kube_pod()
                    .status
                    .clone()
                    .unwrap_or_default()
            };

            let num_statuses = status
                .container_statuses
                .as_ref()
                .map(|statuses| statuses.len())
                .unwrap_or(0);
            let num_init_statuses = status
                .init_container_statuses
                .as_ref()
                .map(|statuses| statuses.len())
                .unwrap_or(0);

            if (num_statuses == num_containers) & (num_init_statuses == num_init_containers) {
                break 'main;
            } else {
                debug!("Pod {} waiting for status to populate: {:?}", &name, status);
                tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
            }
        }
    }

    let mut state: Box<dyn State<PodState>> = Box::new(state);

    loop {
        debug!("Pod {} entering state {:?}", &name, state);

        let latest_pod = { pod.read().await.clone() };

        match state.json_status(pod_state, &latest_pod).await {
            Ok(patch) => {
                patch_status(&api, &name, patch).await;
            }
            Err(e) => {
                warn!("Pod {} status patch returned error: {:?}", &name, e);
            }
        }

        debug!("Pod {} executing state handler {:?}", &name, state);
        let transition = { state.next(pod_state, &latest_pod).await };

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
                    let patch = serde_json::json!(
                        {
                            "metadata": {
                                "resourceVersion": "",
                            },
                            "status": {
                                "phase": Phase::Failed,
                                "reason": format!("{:?}", e),
                            }
                        }
                    );
                    patch_status(&api, &name, patch).await;
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
impl<P: 'static + Sync + Send> State<P> for Stub {
    async fn next(self: Box<Self>, _pod_state: &mut P, _pod: &Pod) -> Transition<P> {
        Transition::Complete(Ok(()))
    }

    async fn json_status(
        &self,
        _pod_state: &mut P,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!(null))
    }
}

#[cfg(test)]
mod test {
    use crate::pod::Pod;
    use crate::state::{State, Transition, TransitionTo};

    #[derive(Debug)]
    struct PodState;

    #[derive(Debug)]
    struct ValidState;

    #[async_trait::async_trait]
    impl State<PodState> for ValidState {
        async fn next(
            self: Box<Self>,
            _pod_state: &mut PodState,
            _pod: &Pod,
        ) -> Transition<PodState> {
            Transition::Complete(Ok(()))
        }

        async fn json_status(
            &self,
            _pod_state: &mut PodState,
            _pod: &Pod,
        ) -> anyhow::Result<serde_json::Value> {
            Ok(serde_json::json!(null))
        }
    }

    #[test]
    fn it_can_transition_to_valid_state() {
        #[derive(Debug)]
        struct TestState;

        impl TransitionTo<ValidState> for TestState {}

        #[async_trait::async_trait]
        impl State<PodState> for TestState {
            async fn next(
                self: Box<Self>,
                _pod_state: &mut PodState,
                _pod: &Pod,
            ) -> Transition<PodState> {
                Transition::next(self, ValidState)
            }

            async fn json_status(
                &self,
                _pod_state: &mut PodState,
                _pod: &Pod,
            ) -> anyhow::Result<serde_json::Value> {
                Ok(serde_json::json!(null))
            }
        }
    }
}
