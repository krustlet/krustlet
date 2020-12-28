use serde::de::DeserializeOwned;

use kube::api::Meta;

use crate::state::{ObjectStatus, ResourceState, SharedState, State};

#[async_trait::async_trait]
pub trait Operator {
    type Manifest: Meta + Clone + DeserializeOwned + Send + 'static + std::fmt::Debug + Sync;
    type Status: ObjectStatus + Send;

    type ResourceState: ResourceState<Manifest = Self::Manifest, Status = Self::Status>;
    type InitialState: State<Self::ResourceState> + Default;
    type DeletedState: State<Self::ResourceState> + Default;

    async fn initialize_resource_state(
        &self,
        manifest: &<Self::ResourceState as ResourceState>::Manifest,
    ) -> anyhow::Result<Self::ResourceState>;

    async fn shared_state(
        &self,
    ) -> SharedState<<Self::ResourceState as ResourceState>::SharedState>;
}
