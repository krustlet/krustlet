use kube::api::Meta;

mod context;
mod operator;
pub mod state;

pub use operator::Operator;

#[derive(Hash, Eq, PartialEq, Clone)]
struct ObjectKey {
    namespace: Option<String>,
    name: String,
}

fn object_key<R: Meta>(object: &R) -> ObjectKey {
    ObjectKey {
        namespace: object.namespace(),
        name: object.name(),
    }
}

pub async fn run_operator<O: Operator>(kubeconfig: &kube::Config, operator: O) {
    let mut context = context::OperatorContext::new(kubeconfig, operator, None);
    context.start().await
}
