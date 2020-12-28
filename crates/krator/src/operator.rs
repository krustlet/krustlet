use serde::de::DeserializeOwned;

use kube::api::Meta;

use crate::object::{ObjectState, ObjectStatus};
use crate::state::{SharedState, State};

#[async_trait::async_trait]
/// Interface for creating an operator.
pub trait Operator: 'static {
    /// Type representing the specification of the object in the Kubernetes API.
    type Manifest: Meta + Clone + DeserializeOwned + Send + 'static + std::fmt::Debug + Sync;

    /// Type describing the status of the object.
    type Status: ObjectStatus + Send;

    /// Type holding data specific to a single object.
    type ObjectState: ObjectState<Manifest = Self::Manifest, Status = Self::Status>;

    /// State handler to run when object is created.
    type InitialState: State<Self::ObjectState> + Default;

    /// State handler to run when object is deleted.
    type DeletedState: State<Self::ObjectState> + Default;

    /// Initialize a new object state for running a new object's state machine.
    async fn initialize_object_state(
        &self,
        manifest: &<Self::ObjectState as ObjectState>::Manifest,
    ) -> anyhow::Result<Self::ObjectState>;

    /// Create a reference to state shared between state machines.
    async fn shared_state(&self) -> SharedState<<Self::ObjectState as ObjectState>::SharedState>;
}
