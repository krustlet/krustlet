//! The pod is backing off after repeated failures and retries.

use crate::state::prelude::*;

use super::registered::Registered;
use super::{BackoffSequence, GenericPodState, GenericProvider};

/// The pod is backing off after repeated failures and retries.
pub struct CrashLoopBackoff<P: GenericProvider> {
    phantom: std::marker::PhantomData<P>,
}

impl<P: GenericProvider> std::fmt::Debug for CrashLoopBackoff<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "CrashLoopBackoff".fmt(formatter)
    }
}

impl<P: GenericProvider> Default for CrashLoopBackoff<P> {
    fn default() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<P: GenericProvider> State<P::ProviderState, P::PodState> for CrashLoopBackoff<P> {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<P::ProviderState>,
        pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> Transition<P::ProviderState, P::PodState> {
        pod_state.backoff(BackoffSequence::CrashLoop).await;
        let next = Registered::<P>::default();
        Transition::next(self, next)
    }

    async fn json_status(
        &self,
        _pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, "CrashLoopBackoff")
    }
}

impl<P: GenericProvider> TransitionTo<Registered<P>> for CrashLoopBackoff<P> {}
