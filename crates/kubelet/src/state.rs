//! Used to define a state machine.
//!
//! Example Pod state machine:
//! ```
//! use kubelet::pod::state::prelude::*;
//! use kubelet::pod::{Pod, Status};
//!
//! #[derive(Debug, TransitionTo)]
//! #[transition_to(TestState)]
//! struct TestState;
//!
//! // Example of manual trait implementation
//! // impl TransitionTo<TestState> for TestState {}
//!
//! struct ProviderState;
//!
//! struct PodState;
//!
//! #[async_trait::async_trait]
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = Status;
//!     type SharedState = ProviderState;
//!     async fn async_drop(self, _provider_state: &mut ProviderState) { }
//! }
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
//!         _provider_state: SharedState<ProviderState>,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> Transition<PodState> {
//!         Transition::next(self, TestState)
//!     }
//!
//!     async fn status(
//!         &self,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> anyhow::Result<PodStatus> {
//!         Ok(Default::default())
//!     }
//! }
//! ```
//!
//! The next state must also be State<PodState>, if it is not State, it fails to compile:
//! ```compile_fail
//! use kubelet::pod::state::prelude::*;
//! use kubelet::pod::Pod;
//!
//! #[derive(Debug, TransitionTo)]
//! #[transition_to(NotState)]
//! struct TestState;
//!
//! struct PodState;
//! struct ProviderState;
//!
//! #[async_trait::async_trait]
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//!     type SharedState = ProviderState;
//!     async fn async_drop(self, _provider_state: &mut ProviderState) { }
//! }
//!
//! #[derive(Debug)]
//! struct NotState;
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
//!         _provider_state: SharedState<ProviderState>,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> Transition<PodState> {
//!         // This fails because NotState is not State
//!         Transition::next(self, NotState)
//!     }
//!
//!     async fn status(
//!         &self,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> anyhow::Result<PodStatus> {
//!         Ok(Default::default())
//!     }
//! }
//! ```
//!
//! Edges must be defined, even for self-transition, with edge removed, compilation fails:
//!
//! ```compile_fail
//! use kubelet::pod::state::prelude::*;
//! use kubelet::pod::Pod;
//!
//! #[derive(Debug)]
//! struct TestState;
//!
//! // impl TransitionTo<TestState> for TestState {}
//!
//! struct PodState;
//! struct ProviderState;
//!
//! #[async_trait::async_trait]
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//!     type SharedState = ProviderState;
//!     async fn async_drop(self, _provider_state: &mut ProviderState) { }
//! }
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
//!         _provider_state: SharedState<ProviderState>,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> Transition<PodState> {
//!         // This fails because TestState is not TransitionTo<TestState>
//!         Transition::next(self, TestState)
//!     }
//!
//!     async fn status(
//!         &self,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> anyhow::Result<PodStatus> {
//!         Ok(Default::default())
//!     }
//! }
//! ```
//!
//! The next state must have the same PodState type, otherwise compilation will fail:
//!
//! ```compile_fail
//! use kubelet::pod::Pod;
//! use kubelet::pod::state::prelude::*;
//!
//! #[derive(Debug, TransitionTo)]
//! #[transition_to(OtherState)]
//! struct TestState;
//!
//! struct PodState;
//! struct ProviderState;
//!
//! #[async_trait::async_trait]
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//!     type SharedState = ProviderState;
//!     async fn async_drop(self, _provider_state: &mut ProviderState) { }
//! }
//!
//! #[derive(Debug)]
//! struct OtherState;
//!
//! struct OtherPodState;
//!
//! #[async_trait::async_trait]
//! impl ResourceState for OtherPodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//!     type SharedState = ProviderState;
//!     async fn async_drop(self, _provider_state: &mut ProviderState) { }
//! }
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
//!         _provider_state: SharedState<ProviderState>,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> Transition<PodState> {
//!         // This fails because `OtherState` is `State<OtherPodState, PodStatus>`
//!         Transition::next(self, OtherState)
//!     }
//!
//!     async fn status(
//!         &self,
//!         _state: &mut PodState,
//!         _pod: &Pod,
//!     ) -> anyhow::Result<PodStatus> {
//!         Ok(Default::default())
//!     }
//! }
//!
//! #[async_trait::async_trait]
//! impl State<OtherPodState> for OtherState {
//!     async fn next(
//!         self: Box<Self>,
//!         _provider_state: SharedState<ProviderState>,
//!         _state: &mut OtherPodState,
//!         _pod: &Pod,
//!     ) -> Transition<OtherPodState> {
//!         Transition::Complete(Ok(()))
//!     }
//!
//!     async fn status(
//!         &self,
//!         _state: &mut OtherPodState,
//!         _pod: &Pod,
//!     ) -> anyhow::Result<PodStatus> {
//!         Ok(Default::default())
//!     }
//! }
//! ```

pub mod common;

#[cfg(feature = "derive")]
#[doc(hidden)]
pub use kubelet_derive::*;

/// Holds arbitrary State objects in Box, and prevents manual construction of Transition::Next
///
/// ```compile_fail
/// use kubelet::state::{Transition, StateHolder, ResourceState};
/// use kubelet::pod::{Pod, Status, state::Stub};
///
/// struct PodState;
/// struct ProviderState;
///
/// #[async_trait::async_trait]
/// impl ResourceState for PodState {
///     type Manifest = Pod;
///     type Status = Status;
///     type SharedState = ProviderState;
///     async fn async_drop(self, _provider_state: &mut ProviderState) { }
/// }
///
/// // This fails because `state` is a private field. Use Transition::next classmethod instead.
/// let _transition = Transition::<PodState>::Next(StateHolder {
///     state: Box::new(Stub),
/// });
/// ```
pub struct StateHolder<S: ResourceState> {
    // This is private, preventing manual construction of Transition::Next
    pub(crate) state: Box<dyn State<S>>,
}

/// Represents result of state execution and which state to transition to next.
pub enum Transition<S: ResourceState> {
    /// Transition to new state.
    Next(StateHolder<S>),
    /// Stop executing the state machine and report the result of the execution.
    Complete(anyhow::Result<()>),
}

/// Mark an edge exists between two states.
pub trait TransitionTo<S> {}

impl<S: ResourceState> Transition<S> {
    // This prevents user from having to box everything AND allows us to enforce edge constraint.
    /// Construct Transition::Next from old state and new state. Both states must be State<PodState>
    /// with matching PodState. Input state must implement TransitionTo<OutputState>, which can be
    /// done manually or with the `TransitionTo` derive macro (requires the `derive` feature to be
    /// enabled)
    #[allow(clippy::boxed_local)]
    pub fn next<I: State<S>, O: State<S>>(_i: Box<I>, o: O) -> Transition<S>
    where
        I: TransitionTo<O>,
    {
        Transition::Next(StateHolder { state: Box::new(o) })
    }

    /// Represents a transition to a new state that is not checked against the
    /// set of permissible transitions. This is intended only for use by generic
    /// states which cannot declare an exit transition to an associated state
    /// without encountering a "conflicting implementations" compiler error.
    #[allow(clippy::boxed_local)]
    pub fn next_unchecked<I: State<S>, O: State<S>>(_i: Box<I>, o: O) -> Transition<S> {
        Transition::Next(StateHolder { state: Box::new(o) })
    }
}

/// Convenience redefinition of Arc<RwLock<T>>
pub type SharedState<T> = std::sync::Arc<tokio::sync::RwLock<T>>;

/// Defines a type which represents a state for a given resource which is passed between its
/// state handlers.
#[async_trait::async_trait]
pub trait ResourceState: 'static + Sync + Send {
    /// The manifest / definition of the resource. Pod, Container, etc.
    type Manifest;
    /// The status type of the state machine.
    type Status;
    /// A type shared between all state machines.
    type SharedState: 'static + Sync + Send;
    /// Clean up resource.
    async fn async_drop(self, shared: &mut Self::SharedState);
}

#[async_trait::async_trait]
/// A trait representing a node in the state graph.
pub trait State<S: ResourceState>: Sync + Send + 'static + std::fmt::Debug {
    /// Provider supplies method to be executed when in this state.
    async fn next(
        self: Box<Self>,
        shared: SharedState<S::SharedState>,
        state: &mut S,
        manifest: &S::Manifest,
    ) -> Transition<S>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn status(&self, state: &mut S, manifest: &S::Manifest) -> anyhow::Result<S::Status>;
}
