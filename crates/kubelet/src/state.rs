//! Used to define a state machine of Pod states.
use log::info;

pub mod default;
#[macro_use]
pub mod macros;

use crate::pod::Pod;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};
use std::sync::Arc;

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
/// A trait representing a node in the state graph.
pub trait State<Provider>: Sync + Send + 'static + std::fmt::Debug {
    /// The next state on success.
    type Success: State<Provider>;
    /// The next state on error.
    type Error: State<Provider>;

    /// Provider supplies method to be executed when in this state.
    async fn next(
        self,
        provider: Arc<Provider>,
        pod: &Pod,
    ) -> anyhow::Result<Transition<Self::Success, Self::Error>>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn json_status(
        &self,
        provider: Arc<Provider>,
        pod: &Pod,
    ) -> anyhow::Result<serde_json::Value>;
}

#[async_recursion::async_recursion]
/// Recursively evaluate state machine until a state returns Complete.
pub async fn run_to_completion<Provider: Send + Sync + 'static>(
    client: kube::Client,
    state: impl State<Provider>,
    provider: Arc<Provider>,
    pod: Pod,
) -> anyhow::Result<()> {
    info!("Pod {} entering state {:?}", pod.name(), state);

    // When handling a new state, we update the Pod state with Kubernetes.
    let api: Api<KubePod> = Api::namespaced(client.clone(), pod.namespace());
    let patch = state.json_status(Arc::clone(&provider), &pod).await?;
    info!("Pod {} status patch: {:?}", pod.name(), &patch);
    let data = serde_json::to_vec(&patch)?;
    api.patch_status(&pod.name(), &PatchParams::default(), data)
        .await?;

    info!("Pod {} executing state handler {:?}", pod.name(), state);
    // Execute state.
    let transition = { state.next(Arc::clone(&provider), &pod).await? };

    info!(
        "Pod {} state execution result: {:?}",
        pod.name(),
        transition
    );

    // Handle transition
    match transition {
        Transition::Advance(s) => run_to_completion(client, s, provider, pod).await,
        Transition::Error(s) => run_to_completion(client, s, provider, pod).await,
        Transition::Complete(result) => result,
    }
}

#[derive(Default, Debug)]
/// Stub state machine for testing.
pub struct Stub;

#[async_trait::async_trait]
impl<P: 'static + Sync + Send> State<P> for Stub {
    type Success = Stub;
    type Error = Stub;

    async fn next(
        self,
        _provider: Arc<P>,
        _pod: &Pod,
    ) -> anyhow::Result<Transition<Self::Success, Self::Error>> {
        Ok(Transition::Complete(Ok(())))
    }

    async fn json_status(
        &self,
        _provider: Arc<P>,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!(null))
    }
}
