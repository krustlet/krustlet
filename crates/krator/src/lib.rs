//! Exposes an API for creating state-machine-based Kubernetes Operators.

#![deny(missing_docs)]

mod manager;
mod manifest;
mod object;
mod operator;
mod runtime;
mod store;
pub mod util;

#[cfg(feature = "admission-webhook")]
pub mod admission;

pub mod state;

// TODO: Remove once webhooks are supported.
#[cfg(not(feature = "admission-webhook"))]
pub use manager::controller::{ControllerBuilder, Watchable};
#[cfg(not(feature = "admission-webhook"))]
pub use manager::Manager;

pub use manifest::Manifest;
pub use object::{ObjectState, ObjectStatus};
pub use operator::Operator;
pub use runtime::OperatorRuntime;
pub use state::{SharedState, State, Transition, TransitionTo};
pub use store::Store;

#[cfg(feature = "derive")]
#[allow(unused_imports)]
#[macro_use]
extern crate krator_derive;

#[cfg(feature = "derive")]
#[doc(hidden)]
pub use krator_derive::*;
