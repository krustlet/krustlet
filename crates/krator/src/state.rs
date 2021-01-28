//! Used to define a state machine.

use k8s_openapi::Resource;
use kube::api::{Meta, PatchParams};
use kube::Api;
use log::{debug, error, trace, warn};
use serde::de::DeserializeOwned;

use crate::object::ObjectStatus;
use crate::Manifest;
// Re-export for compatibility.
pub use crate::object::ObjectState as ResourceState;

/// Guard for preventing manual construction on Transition::Next.
pub struct StateHolder<S: ResourceState> {
    pub(crate) state: Box<dyn State<S>>,
}

impl<S: ResourceState> From<StateHolder<S>> for Box<dyn State<S>> {
    fn from(holder: StateHolder<S>) -> Box<dyn State<S>> {
        holder.state
    }
}

/// Represents result of state execution and which state to transition to next.
pub enum Transition<S: ResourceState> {
    /// Transition to new state.
    Next(StateHolder<S>),
    /// Stop executing the state machine and report the result of the execution.
    Complete(anyhow::Result<()>),
}

/// Mark an edge exists between two states.
pub trait TransitionTo<S> {}

impl<S: ResourceState> Transition<S> {
    // This prevents user from having to box everything AND allows us to enforce edge constraint.
    /// Construct Transition::Next from old state and new state. Both states must be State<PodState>
    /// with matching PodState. Input state must implement TransitionTo<OutputState>, which can be
    /// done manually or with the `TransitionTo` derive macro (requires the `derive` feature to be
    /// enabled)
    #[allow(clippy::boxed_local)]
    pub fn next<I: State<S>, O: State<S>>(_i: Box<I>, o: O) -> Transition<S>
    where
        I: TransitionTo<O>,
    {
        Transition::Next(StateHolder { state: Box::new(o) })
    }

    /// Represents a transition to a new state that is not checked against the
    /// set of permissible transitions. This is intended only for use by generic
    /// states which cannot declare an exit transition to an associated state
    /// without encountering a "conflicting implementations" compiler error.
    #[allow(clippy::boxed_local)]
    pub fn next_unchecked<I: State<S>, O: State<S>>(_i: Box<I>, o: O) -> Transition<S> {
        Transition::Next(StateHolder { state: Box::new(o) })
    }
}

/// Convenience redefinition of Arc<RwLock<T>>
pub type SharedState<T> = std::sync::Arc<tokio::sync::RwLock<T>>;

#[async_trait::async_trait]
/// A trait representing a node in the state graph.
pub trait State<S: ResourceState>: Sync + Send + 'static + std::fmt::Debug {
    /// Provider supplies method to be executed when in this state.
    async fn next(
        self: Box<Self>,
        shared: SharedState<S::SharedState>,
        state: &mut S,
        manifest: Manifest<S::Manifest>,
    ) -> Transition<S>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn status(&self, state: &mut S, manifest: &S::Manifest) -> anyhow::Result<S::Status>;
}

/// Iteratively evaluate state machine until it returns Complete.
pub async fn run_to_completion<S: ResourceState>(
    client: &kube::Client,
    state: impl State<S>,
    shared: SharedState<S::SharedState>,
    object_state: &mut S,
    manifest: Manifest<S::Manifest>,
) where
    S::Manifest: Resource + Meta + DeserializeOwned,
    S::Status: ObjectStatus,
{
    let (name, namespace, api) = {
        let initial_manifest = manifest.latest();
        let namespace = initial_manifest.namespace();
        let name = initial_manifest.name();

        let api: Api<S::Manifest> = match namespace {
            Some(ref namespace) => Api::namespaced(client.clone(), namespace),
            None => Api::all(client.clone()),
        };
        (name, namespace, api)
    };

    let mut state: Box<dyn State<S>> = Box::new(state);

    loop {
        debug!(
            "Object {} in namespace {:?} entering state {:?}",
            &name, &namespace, state
        );

        let latest_manifest = manifest.latest();

        match state.status(object_state, &latest_manifest).await {
            Ok(status) => {
                patch_status(&api, &name, status).await;
            }
            Err(e) => {
                warn!(
                    "Object {} in namespace {:?} status patch returned error: {:?}",
                    &name, &namespace, e
                );
            }
        }

        trace!(
            "Object {} in namespace {:?} executing state handler {:?}",
            &name,
            &namespace,
            state
        );
        let transition = {
            state
                .next(shared.clone(), object_state, manifest.clone())
                .await
        };

        state = match transition {
            Transition::Next(s) => {
                let state = s.into();
                trace!(
                    "Object {} in namespace {:?} transitioning to {:?}.",
                    &name,
                    &namespace,
                    state
                );
                state
            }
            Transition::Complete(result) => match result {
                Ok(()) => {
                    debug!(
                        "Object {} in namespace {:?} state machine exited without error",
                        &name, &namespace
                    );
                    break;
                }
                Err(e) => {
                    error!(
                        "Object {} in namespace {:?} state machine exited with error: {:?}",
                        &name, &namespace, e
                    );
                    let status = S::Status::failed(&format!("{:?}", e));
                    patch_status(&api, &name, status).await;
                    break;
                }
            },
        };
    }
}

/// Patch object status with Kubernetes API.
pub async fn patch_status<R: Resource + Clone + DeserializeOwned, S: ObjectStatus>(
    api: &Api<R>,
    name: &str,
    status: S,
) {
    let patch = status.json_patch();
    match serde_json::to_string(&patch) {
        Ok(s) => {
            debug!("Applying status patch to object {}: '{}'", &name, &s);
            match api
                .patch_status(&name, &PatchParams::default(), s.as_bytes().to_vec())
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    warn!("Object {} error patching status: {:?}", name, e);
                }
            }
        }
        Err(e) => {
            warn!(
                "Object {} error serializing status patch {:?}: {:?}",
                name, &patch, e
            );
        }
    }
}
