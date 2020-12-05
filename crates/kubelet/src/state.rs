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
//! struct PodState;
//!
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = Status;
//! }
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
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
//!
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//! }
//!
//! #[derive(Debug)]
//! struct NotState;
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
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
//!
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//! }
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
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
//!
//! impl ResourceState for PodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//! }
//!
//! #[derive(Debug)]
//! struct OtherState;
//!
//! struct OtherPodState;
//!
//! impl ResourceState for OtherPodState {
//!     type Manifest = Pod;
//!     type Status = PodStatus;
//! }
//!
//! #[async_trait::async_trait]
//! impl State<PodState> for TestState {
//!     async fn next(
//!         self: Box<Self>,
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
pub mod prelude;

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
///
/// impl ResourceState for PodState {
///     type Manifest = Pod;
///     type Status = Status;
/// }
///
/// // This fails because `state` is a private field. Use Transition::next classmethod instead.
/// let _transition = Transition::<PodState>::Next(StateHolder {
///     state: Box::new(Stub),
/// });
/// ```
pub struct StateHolder<ProviderState, S: ResourceState> {
    // This is private, preventing manual construction of Transition::Next
    state: Box<dyn State<ProviderState, S>>,
}

/// Represents result of state execution and which state to transition to next.
pub enum Transition<ProviderState, S: ResourceState> {
    /// Transition to new state.
    Next(StateHolder<ProviderState, S>),
    /// Stop executing the state machine and report the result of the execution.
    Complete(anyhow::Result<()>),
}

/// Mark an edge exists between two states.
pub trait TransitionTo<S> {}

impl<ProviderState, S: ResourceState> Transition<ProviderState, S> {
    // This prevents user from having to box everything AND allows us to enforce edge constraint.
    /// Construct Transition::Next from old state and new state. Both states must be State<PodState>
    /// with matching PodState. Input state must implement TransitionTo<OutputState>, which can be
    /// done manually or with the `TransitionTo` derive macro (requires the `derive` feature to be
    /// enabled)
    #[allow(clippy::boxed_local)]
    pub fn next<I: State<ProviderState, S: ResourceState>, O: State<ProviderState, S>>(
        _i: Box<I>,
        o: O,
    ) -> Transition<ProviderState, S>
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
    pub fn next_unchecked<I: State<ProviderState, PodState>, S: State<ProviderState, PodState>>(
        _i: Box<I>,
        s: S,
    ) -> Transition<ProviderState, PodState> {
        Transition::Next(StateHolder { state: Box::new(s) })
    }
}

// TODO: consider moving this to the ProviderState
#[async_trait::async_trait]
/// Allow for asynchronous cleanup up of PodState.
pub trait AsyncDrop: Sized {
    /// The type of any provider-level state from which the pod
    /// needs to be cleaned up.
    type ProviderState;
    /// Clean up PodState.
    async fn async_drop(self, provider_state: &mut Self::ProviderState);
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

    /// Acquires a read lock for the shared state.
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, T> {
        self.state.read().await
    }

    /// Acquires a write lock for the shared state.
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, T> {
        self.state.write().await
    }
}

/// Defines a type which represents a state for a given resource which is passed between its
/// state handlers.
pub trait ResourceState {
    /// The manifest / definition of the resource. Pod, Container, etc.
    type Manifest;
    /// The status type of the state machine.
    type Status;
}

#[async_trait::async_trait]
/// A trait representing a node in the state graph.
pub trait State<ProviderState, S: ResourceState>: Sync + Send + 'static + std::fmt::Debug {
    /// Provider supplies method to be executed when in this state.
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut S,
        pod: &Pod,
    ) -> Transition<ProviderState, S>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn status(&self, state: &mut S, manifest: &S::Manifest) -> anyhow::Result<S::Status>;
}
