use serde::de::DeserializeOwned;

use kube::api::Meta;

use crate::object::{ObjectStatus, ResourceState};
use crate::state::{SharedState, State};

#[async_trait::async_trait]
/// Interface for creating an operator.
pub trait Operator {
    /// Type representing the specification of the object in the Kubernetes API.
    type Manifest: Meta + Clone + DeserializeOwned + Send + 'static + std::fmt::Debug + Sync;

    /// Type describing the status of the object.
    type Status: ObjectStatus + Send;

    /// Type holding state specific to a single object.
    type ResourceState: ResourceState<Manifest = Self::Manifest, Status = Self::Status>;

    /// State handler to run when object is created.
    type InitialState: State<Self::ResourceState> + Default;

    /// State handler to run when object is deleted.
    type DeletedState: State<Self::ResourceState> + Default;

    /// Initialize a new resource state for running a new object's state machine.
    async fn initialize_resource_state(
        &self,
        manifest: &<Self::ResourceState as ResourceState>::Manifest,
    ) -> anyhow::Result<Self::ResourceState>;

    /// Create a reference to state shared between state machines.
    async fn shared_state(
        &self,
    ) -> SharedState<<Self::ResourceState as ResourceState>::SharedState>;
}
