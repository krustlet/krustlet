//! Some imports that are used when implementing Kubelet state handlers.

pub use crate::pod::{
    make_registered_status, make_status, make_status_with_containers, Phase, Pod,
};
pub use crate::state::{SharedState, State, Transition, TransitionTo};
