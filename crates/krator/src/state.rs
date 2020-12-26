//! Used to define a state machine.

pub struct StateHolder<S: ResourceState> {
    // This is private, preventing manual construction of Transition::Next
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

/// Defines a type which represents a state for a given resource which is passed between its
/// state handlers.
#[async_trait::async_trait]
pub trait ResourceState: 'static + Sync + Send {
    /// The manifest / definition of the resource. Pod, Container, etc.
    type Manifest;
    /// The status type of the state machine.
    type Status;
    /// A type shared between all state machines.
    type SharedState: 'static + Sync + Send;
    /// Clean up resource.
    async fn async_drop(self, shared: &mut Self::SharedState);
}

#[async_trait::async_trait]
/// A trait representing a node in the state graph.
pub trait State<S: ResourceState>: Sync + Send + 'static + std::fmt::Debug {
    /// Provider supplies method to be executed when in this state.
    async fn next(
        self: Box<Self>,
        shared: SharedState<S::SharedState>,
        state: &mut S,
        manifest: &S::Manifest,
    ) -> Transition<S>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn status(&self, state: &mut S, manifest: &S::Manifest) -> anyhow::Result<S::Status>;
}
