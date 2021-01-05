//! Exposes an API for creating state-machine-based Kubernetes Operators.

#![deny(missing_docs)]

mod object;
mod operator;
mod runtime;

pub mod state;

pub use object::{ObjectState, ObjectStatus};
pub use operator::Operator;
pub use runtime::OperatorRuntime;
