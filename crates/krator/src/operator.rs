use kube::Resource;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;

use crate::object::{ObjectState, ObjectStatus};
use crate::state::{SharedState, State};
use crate::Manifest;

#[async_trait::async_trait]
/// Interface for creating an operator.
pub trait Operator: 'static + Sync + Send {
    /// Type representing the specification of the object in the Kubernetes API.
    type Manifest: Resource<DynamicType = ()>
        + Clone
        + DeserializeOwned
        + Serialize
        + Send
        + 'static
        + Debug
        + Sync
        + Default
        + std::marker::Unpin;

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
        manifest: &Self::Manifest,
    ) -> anyhow::Result<Self::ObjectState>;

    /// Create a reference to state shared between state machines.
    async fn shared_state(&self) -> SharedState<<Self::ObjectState as ObjectState>::SharedState>;

    /// Called before the state machine is run.
    async fn registration_hook(
        &self,
        mut _manifest: Manifest<Self::Manifest>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "admission-webhook")]
    /// Invoked when object is created or modified. Can mutate the and / or deny the request.
    async fn admission_hook(
        &self,
        manifest: Self::Manifest,
    ) -> crate::admission::AdmissionResult<Self::Manifest>;

    /// Called before the state machine is run.
    async fn deregistration_hook(
        &self,
        mut _manifest: Manifest<Self::Manifest>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
