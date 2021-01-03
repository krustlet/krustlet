//! Exposes an API for creating state-machine-based Kubernetes Operators.

#![deny(missing_docs)]

mod context;
mod object;
mod operator;

pub mod state;

pub use context::OperatorContext;
pub use object::{ObjectState, ObjectStatus};
pub use operator::Operator;
