use kube::api::Meta;

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct ObjectKey {
    namespace: Option<String>,
    name: String,
}

impl ObjectKey {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn namespace(&self) -> Option<&String> {
        self.namespace.as_ref()
    }
}

impl<R: Meta> From<&R> for ObjectKey {
    fn from(object: &R) -> ObjectKey {
        ObjectKey {
            namespace: object.namespace(),
            name: object.name(),
        }
    }
}

/// Defines a type which represents a state for a given resource which is passed between its
/// state handlers.
#[async_trait::async_trait]
pub trait ResourceState: 'static + Sync + Send {
    /// The manifest / definition of the resource. Pod, Container, etc.
    type Manifest: Clone;
    /// The status type of the state machine.
    type Status;
    /// A type shared between all state machines.
    type SharedState: 'static + Sync + Send;
    /// Clean up resource.
    async fn async_drop(self, shared: &mut Self::SharedState);
}

/// Interfacefor types which represent the status of an object.
pub trait ObjectStatus {
    /// Produce a JSON patch based on the set values of this status.
    fn json_patch(&self) -> serde_json::Value;
    /// Produce a status which marks an object as failed with supplied error message.
    fn failed(e: &str) -> Self;
}
