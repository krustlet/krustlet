//! Used to define a state machine of Pod states.
use log::{debug, error, warn};

pub mod common;
pub mod prelude;

use crate::pod::{initialize_pod_container_statuses, patch_status};
use crate::pod::{Phase, Pod};
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::Api;
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
pub struct StateHolder<ProviderState, PodState> {
    // This is private, preventing manual construction of Transition::Next
    state: Box<dyn State<ProviderState, PodState>>,
}

/// Represents result of state execution and which state to transition to next.
pub enum Transition<ProviderState, PodState> {
    /// Transition to new state.
    Next(StateHolder<ProviderState, PodState>),
    /// Stop executing the state machine and report the result of the execution.
    Complete(anyhow::Result<()>),
}

/// Mark an edge exists between two states.
pub trait TransitionTo<S> {}

impl<ProviderState, PodState> Transition<ProviderState, PodState> {
    // This prevents user from having to box everything AND allows us to enforce edge constraint.
    /// Construct Transition::Next from old state and new state. Both states must be State<PodState>
    /// with matching PodState. Input state must implement TransitionTo<OutputState>, which can be
    /// done manually or with the `TransitionTo` derive macro (requires the `derive` feature to be
    /// enabled)
    ///
    /// ```
    /// use kubelet::state::{Transition, State, TransitionTo, SharedState};
    /// use kubelet::pod::Pod;
    ///
    /// #[derive(Debug, TransitionTo)]
    /// #[transition_to(TestState)]
    /// struct TestState;
    ///
    /// // Example of manual trait implementation
    /// // impl TransitionTo<TestState> for TestState {}
    ///
    /// struct ProviderState;
    /// struct PodState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<ProviderState, PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _provider_state: SharedState<ProviderState>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> Transition<ProviderState, PodState> {
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
    /// struct ProviderState;
    /// struct PodState;
    ///
    /// #[derive(Debug)]
    /// struct NotState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<ProviderState, PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _provider_state: SharedState<ProviderState>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> Transition<ProviderState, PodState> {
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
    /// struct ProviderState;
    /// struct PodState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<ProviderState, PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _provider_state: SharedState<ProviderState>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> Transition<ProviderState, PodState> {
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
    /// struct ProviderState;
    /// struct PodState;
    ///
    /// #[derive(Debug)]
    /// struct OtherState;
    ///
    /// struct OtherPodState;
    ///
    /// #[async_trait::async_trait]
    /// impl State<ProviderState, PodState> for TestState {
    ///     async fn next(
    ///         self: Box<Self>,
    ///         _provider_state: SharedState<ProviderState>,
    ///         _pod_state: &mut PodState,
    ///         _pod: &Pod,
    ///     ) -> Transition<ProviderState, PodState> {
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
    ///         _provider_state: SharedState<ProviderState>,
    ///         _pod_state: &mut OtherPodState,
    ///         _pod: &Pod,
    ///     ) -> Transition<ProviderState, OtherPodState> {
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
    pub fn next<I: State<ProviderState, PodState>, S: State<ProviderState, PodState>>(
        _i: Box<I>,
        s: S,
    ) -> Transition<ProviderState, PodState>
    where
        I: TransitionTo<S>,
    {
        Transition::Next(StateHolder { state: Box::new(s) })
    }

    /// Represents a transition to a new state that is not checked against the
    /// set of permissible transitions. This is intended only for use by generic
    /// states which cannot declare an exit transition to an associated state
    /// without encountering a "conflicting implementations" compiler error.
    #[allow(clippy::boxed_local)]
    pub fn next_unchecked<I: State<ProviderState, PodState>, S: State<ProviderState, PodState>>(
        _i: Box<I>,
        s: S,
    ) -> Transition<ProviderState, PodState> {
        Transition::Next(StateHolder { state: Box::new(s) })
    }
}

#[async_trait::async_trait]
/// Allow for asynchronous cleanup up of PodState.
pub trait AsyncDrop: Sized {
    /// Clean up PodState.
    async fn async_drop(self);
}

/// Provides shared access to provider-level state between multiple pod
/// state machines running within the provider.
pub struct SharedState<T> {
    state: std::sync::Arc<tokio::sync::RwLock<T>>,
}

impl<T> Clone for SharedState<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<T> SharedState<T> {
    /// Creates a SharedState to provide shared access to the specified value.
    pub fn new(value: T) -> Self {
        Self {
            state: std::sync::Arc::<_>::new(tokio::sync::RwLock::new(value)),
        }
    }
}

#[async_trait::async_trait]
/// A trait representing a node in the state graph.
pub trait State<ProviderState, PodState>: Sync + Send + 'static + std::fmt::Debug {
    /// Provider supplies method to be executed when in this state.
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<ProviderState, PodState>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn json_status(
        &self,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<serde_json::Value>;
}

/// Iteratively evaluate state machine until it returns Complete.
pub async fn run_to_completion<
    ProviderState: Send + Sync + 'static,
    PodState: Send + Sync + 'static,
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

        match state.json_status(pod_state, &latest_pod).await {
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
impl<Pr: 'static + Sync + Send, P: 'static + Sync + Send> State<Pr, P> for Stub {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<Pr>,
        _pod_state: &mut P,
        _pod: &Pod,
    ) -> Transition<Pr, P> {
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
        impl State<ProviderState, PodState> for TestState {
            async fn next(
                self: Box<Self>,
                _provider_state: SharedState<ProviderState>,
                _pod_state: &mut PodState,
                _pod: &Pod,
            ) -> Transition<ProviderState, PodState> {
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
