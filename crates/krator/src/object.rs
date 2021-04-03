use kube::api::Resource;

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

impl<R: Resource> From<&R> for ObjectKey {
    fn from(object: &R) -> ObjectKey {
        ObjectKey {
            namespace: object.namespace(),
            name: object.name(),
        }
    }
}

/// Interface for types which capture data related to a specific object that is
/// passed between the object's state handlers.  
#[async_trait::async_trait]
pub trait ObjectState: 'static + Sync + Send {
    /// The manifest / definition of the resource. Pod, Custom Resource, etc.
    /// This does not need to implement `Resource` or `Meta`, but if it does
    /// not then you will not be able to use it with `Operator` and will have
    /// to write your own `state::run_to_completion` method.
    type Manifest: Clone + Sync + Send + std::marker::Unpin + 'static;
    /// The status type of the state machine.
    type Status;
    /// A type representing data shared between all state machines.
    type SharedState: 'static + Sync + Send;
    /// Clean up any resources when this object is deleted.
    async fn async_drop(self, shared: &mut Self::SharedState);
}

/// Interface for types which represent the Kubernetes status of an object.
pub trait ObjectStatus {
    /// Produce a JSON patch based on this status.
    /// You generally want to keep track of which fields are set and only update those.
    /// Given Kubernetes' distributed nature, it is difficult to know the exact state
    /// of the status that you are patching. Be especially careful with lists that may
    /// change ordering and fields which may default to null and must first be initialized
    /// before they can be appended to or spliced.
    fn json_patch(&self) -> serde_json::Value;
    /// Produce a status which marks an object as failed with supplied error message.
    /// This can mean different things for different resources and will be used to emit
    /// an error if the state machine does not exit gracefully.
    fn failed(e: &str) -> Self;
}
