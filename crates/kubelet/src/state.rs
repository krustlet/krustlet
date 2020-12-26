//! Re-export of `krator::state` and common states for Kublets.
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
pub use krator::state::*;
pub mod common;

#[cfg(feature = "derive")]
#[doc(hidden)]
pub use kubelet_derive::*;
