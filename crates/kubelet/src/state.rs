//! Used to define a state machine of Pod states.
use log::debug;

// pub mod default;
#[macro_use]
pub mod macros;
pub mod prelude;

use crate::pod::Pod;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};

/// Represents result of state execution and which state to transition to next.
#[derive(Debug)]
pub enum Transition<S, E> {
    /// Advance to next node.
    Advance(S),
    /// Transition to error node.
    Error(E),
    /// This is a terminal node of the state graph.
    Complete(anyhow::Result<()>),
}

#[async_trait::async_trait]
/// Allow for asynchronous cleanup up of PodState.
pub trait AsyncDrop: Sized {
    /// Clean up PodState.
    async fn async_drop(self);
}

#[async_trait::async_trait]
/// A trait representing a node in the state graph.
pub trait State<PodState>: Sync + Send + 'static + std::fmt::Debug {
    /// Provider supplies method to be executed when in this state.
    async fn next(
        &self,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<Transition<Box<dyn State<PodState>>, Box<dyn State<PodState>>>>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn json_status(
        &self,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<serde_json::Value>;
}

/// Iteratively evaluate state machine until it returns Complete.
pub async fn run_to_completion<PodState: Send + Sync + 'static>(
    client: &kube::Client,
    state: impl State<PodState>,
    pod_state: &mut PodState,
    pod: &Pod,
) -> anyhow::Result<()> {
    let api: Api<KubePod> = Api::namespaced(client.clone(), pod.namespace());

    let mut state: Box<dyn State<PodState>> = Box::new(state);

    loop {
        debug!("Pod {} entering state {:?}", pod.name(), state);

        let patch = state.json_status(pod_state, &pod).await?;
        debug!("Pod {} status patch: {:?}", pod.name(), &patch);
        let data = serde_json::to_vec(&patch)?;
        api.patch_status(&pod.name(), &PatchParams::default(), data)
            .await?;
        debug!("Pod {} executing state handler {:?}", pod.name(), state);

        let transition = { state.next(pod_state, &pod).await? };

        debug!(
            "Pod {} state execution result: {:?}",
            pod.name(),
            transition
        );

        state = match transition {
            Transition::Advance(s) => s,
            Transition::Error(s) => s,
            Transition::Complete(result) => break result,
        };
    }
}

#[derive(Default, Debug)]
/// Stub state machine for testing.
pub struct Stub;

#[async_trait::async_trait]
impl<P: 'static + Sync + Send> State<P> for Stub {
    async fn next(
        &self,
        _pod_state: &mut P,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<Box<dyn State<P>>, Box<dyn State<P>>>> {
        Ok(Transition::Complete(Ok(())))
    }

    async fn json_status(
        &self,
        _pod_state: &mut P,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!(null))
    }
}
