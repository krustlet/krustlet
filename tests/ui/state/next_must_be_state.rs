// Test that State<T> can only transition to State<T>
// edition:2018
extern crate async_trait;
extern crate kubelet;
extern crate anyhow;

use kubelet::pod::state::prelude::*;
use kubelet::pod::Pod;

#[derive(Debug, TransitionTo)]
#[transition_to(NotState)]
struct TestState;

struct PodState;
struct ProviderState;

#[async_trait::async_trait]
impl ResourceState for PodState {
    type Manifest = Pod;
    type Status = PodStatus;
    type SharedState = ProviderState;
    async fn async_drop(self, _provider_state: &mut ProviderState) { }
}

#[derive(Debug)]
struct NotState;

#[async_trait::async_trait]
impl State<PodState> for TestState {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<ProviderState>,
        _state: &mut PodState,
        _pod: Manifest<Pod>,
    ) -> Transition<PodState> {
        // This fails because NotState is not State
        Transition::next(self, NotState)
    }

    async fn status(
        &self,
        _state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<PodStatus> {
        Ok(Default::default())
    }
}

fn main() {}
