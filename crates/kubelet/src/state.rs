//! Re-export of `krator::state` and common states for Kubelets.
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
//! impl ObjectState for PodState {
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
//!         _pod: Manifest<Pod>,
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

pub mod common;

#[cfg(feature = "derive")]
#[doc(hidden)]
pub use krator::TransitionTo;
