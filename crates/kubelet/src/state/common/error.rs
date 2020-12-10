//! The Pod failed to run.

use crate::pod::state::prelude::*;
use super::crash_loop_backoff::CrashLoopBackoff;
use super::registered::Registered;
use super::{GenericPodState, GenericProvider, ThresholdTrigger};

/// The Pod failed to run.
pub struct Error<P: GenericProvider> {
    phantom: std::marker::PhantomData<P>,
    message: String,
}

impl<P: GenericProvider> std::fmt::Debug for Error<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = format!("Error: {}", self.message);
        text.fmt(formatter)
    }
}

impl<P: GenericProvider> Error<P> {
    /// Creates an instance of the Error state.
    pub fn new(message: String) -> Self {
        Self {
            phantom: std::marker::PhantomData,
            message,
        }
    }
}

#[async_trait::async_trait]
impl<P: GenericProvider> State<P::ProviderState, P::PodState> for Error<P> {
    async fn next(
        self: Box<Self>,
        _provider_state: SharedState<P::ProviderState>,
        pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> Transition<P::ProviderState, P::PodState> {
        match pod_state.record_error() {
            ThresholdTrigger::Triggered => {
                let next = CrashLoopBackoff::<P>::default();
                Transition::next(self, next)
            }
            ThresholdTrigger::Untriggered => {
                tokio::time::delay_for(std::time::Duration::from_secs(5)).await;
                let next = Registered::<P>::default();
                Transition::next(self, next)
            }
        }
    }

    async fn status(
        &self,
        _pod_state: &mut P::PodState,
        _pod: &Pod,
    ) -> anyhow::Result<<P::PodState as ResourceState>::Status> {
        Ok(make_status(Phase::Pending, &self.message))
    }
}

impl<P: GenericProvider> TransitionTo<CrashLoopBackoff<P>> for Error<P> {}
impl<P: GenericProvider> TransitionTo<Registered<P>> for Error<P> {}
