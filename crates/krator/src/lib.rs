//! Exposes an API for creating state-machine-based Kubernetes Operators.

#![deny(missing_docs)]

mod context;
mod object;
mod operator;

pub mod state;

pub use operator::Operator;

/// Run Operator forever.
pub async fn run_operator<O: Operator>(kubeconfig: &kube::Config, operator: O) {
    let mut context = context::OperatorContext::new(kubeconfig, operator, None);
    context.start().await
}
