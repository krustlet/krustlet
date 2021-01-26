//! The pod is backing off after repeated failures and retries.

use super::registered::Registered;
use super::{BackoffSequence, GenericPodState, GenericProvider};
use crate::pod::state::prelude::*;

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
impl<P: GenericProvider> State<P::PodState> for CrashLoopBackoff<P> {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<P::ProviderState>,
        pod_state: &mut P::PodState,
        _pod: Manifest<Pod>,
    ) -> Transition<P::PodState> {
        pod_state.backoff(BackoffSequence::CrashLoop).await;
        let next = Registered::<P>::default();
        Transition::next(self, next)
    }

    async fn status(&self, _pod_state: &mut P::PodState, _pod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Pending, "CrashLoopBackoff"))
    }
}

impl<P: GenericProvider> TransitionTo<Registered<P>> for CrashLoopBackoff<P> {}
