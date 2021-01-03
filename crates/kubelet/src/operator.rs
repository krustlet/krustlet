use crate::pod::Pod;
use crate::provider::Provider;
use krator::state::SharedState;
use krator::ObjectState;
use krator::Operator;
use std::sync::Arc;

pub(crate) struct PodOperator<P: Provider> {
    provider: Arc<P>,
}

impl<P: Provider> PodOperator<P> {
    pub fn new(provider: Arc<P>) -> Self {
        PodOperator { provider }
    }
}

#[async_trait::async_trait]
impl<P: 'static + Provider + Send + Sync> Operator for PodOperator<P> {
    type Manifest = crate::pod::Pod;
    type Status = crate::pod::Status;
    type ObjectState = P::PodState;
    type InitialState = P::InitialState;
    type DeletedState = P::TerminatedState;
    async fn initialize_object_state(&self, manifest: &Pod) -> anyhow::Result<P::PodState> {
        self.provider.initialize_pod_state(manifest).await
    }

    async fn shared_state(&self) -> SharedState<<P::PodState as ObjectState>::SharedState> {
        todo!()
    }
}
