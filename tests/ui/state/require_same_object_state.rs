// Test that both states must have the same ObjectState. 
// edition:2018
extern crate async_trait;
extern crate kubelet;
extern crate anyhow;

use kubelet::pod::Pod;
use kubelet::pod::state::prelude::*;

#[derive(Debug, TransitionTo)]
#[transition_to(OtherState)]
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
struct OtherState;

struct OtherPodState;

#[async_trait::async_trait]
impl ResourceState for OtherPodState {
    type Manifest = Pod;
    type Status = PodStatus;
    type SharedState = ProviderState;
    async fn async_drop(self, _provider_state: &mut ProviderState) { }
}

#[async_trait::async_trait]
impl State<PodState> for TestState {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<ProviderState>,
        _state: &mut PodState,
        _pod: Manifest<Pod>,
    ) -> Transition<PodState> {
        // This fails because `OtherState` is `State<OtherPodState, PodStatus>`
        Transition::next(self, OtherState)
    }

    async fn status(
        &self,
        _state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<PodStatus> {
        Ok(Default::default())
    }
}

#[async_trait::async_trait]
impl State<OtherPodState> for OtherState {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<ProviderState>,
        _state: &mut OtherPodState,
        _pod: Manifest<Pod>,
    ) -> Transition<OtherPodState> {
        Transition::Complete(Ok(()))
    }

    async fn status(
        &self,
        _state: &mut OtherPodState,
        _pod: &Pod,
    ) -> anyhow::Result<PodStatus> {
        Ok(Default::default())
    }
}

fn main() {}
