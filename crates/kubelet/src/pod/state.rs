//! Functions for running Pod state machines.
use crate::pod::{Pod, Status as PodStatus};
use krator::{Manifest, ObjectState, SharedState, State, Transition};

/// Prelude for Pod state machines.
pub mod prelude {
    pub use crate::pod::{
        make_status, make_status_with_containers, status::StatusBuilder, Phase, Pod,
        Status as PodStatus,
    };
    pub use krator::{Manifest, ObjectState, SharedState, State, Transition, TransitionTo};
}

#[derive(Default, Debug)]
/// Stub state machine for testing.
pub struct Stub;

#[async_trait::async_trait]
impl<PodState: ObjectState<Manifest = Pod, Status = PodStatus>> State<PodState> for Stub {
    async fn next(
        self: Box<Self>,
        _shared_state: SharedState<PodState::SharedState>,
        _pod_state: &mut PodState,
        _pod: Manifest<Pod>,
    ) -> Transition<PodState> {
        Transition::Complete(Ok(()))
    }

    async fn status(&self, _state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(Default::default())
    }
}

#[cfg(test)]
mod test {
    use crate::pod::state::prelude::*;
    use crate::pod::{Pod, Status as PodStatus};
    use krator::Manifest;

    #[derive(Debug)]
    struct ProviderState;

    #[derive(Debug)]
    struct PodState;

    #[derive(Debug)]
    struct ValidState;

    #[async_trait::async_trait]
    impl ObjectState for PodState {
        type Manifest = Pod;
        type Status = PodStatus;
        type SharedState = ProviderState;
        async fn async_drop(self, _shared_state: &mut Self::SharedState) {}
    }

    #[async_trait::async_trait]
    impl State<PodState> for ValidState {
        async fn next(
            self: Box<Self>,
            _provider_state: SharedState<ProviderState>,
            _pod_state: &mut PodState,
            _pod: Manifest<Pod>,
        ) -> Transition<PodState> {
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
        impl State<PodState> for TestState {
            async fn next(
                self: Box<Self>,
                _provider_state: SharedState<ProviderState>,
                _pod_state: &mut PodState,
                _pod: Manifest<Pod>,
            ) -> Transition<PodState> {
                Transition::next(self, ValidState)
            }

            async fn status(&self, _state: &mut PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
                Ok(Default::default())
            }
        }
    }
}
