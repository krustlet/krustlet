use crate::pod::Pod;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Represents result of state execution and which state to transition to next.
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
pub trait State<ProviderState: Sync + Send + 'static>: Sync + Send + 'static {
    /// The next state on success.
    type Success: State<ProviderState>;
    /// The next state on error.
    type Error: State<ProviderState>;

    /// Provider supplies method to be executed when in this state.
    async fn next(
        self,
        state: Arc<Mutex<ProviderState>>,
        pod: &Pod,
    ) -> anyhow::Result<Transition<Self::Success, Self::Error>>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn json_status(
        &self,
        state: &ProviderState,
        pod: &Pod,
    ) -> anyhow::Result<serde_json::Value>;
}

#[async_recursion::async_recursion]
/// Recursively evaluate state machine until a state returns Complete.
pub async fn run_to_completion<ProviderState: Send + Sync + 'static>(
    client: &kube::Client,
    state: impl State<ProviderState>,
    provider_state: Arc<Mutex<ProviderState>>,
    pod: Pod,
) -> anyhow::Result<()> {
    // When handling a new state, we update the Pod state with Kubernetes.
    let api: Api<KubePod> = Api::namespaced(client.clone(), pod.namespace());
    let patch = {
        let pstate = provider_state.lock().await;
        state.json_status(&(*pstate), &pod).await?
    };
    let data = serde_json::to_vec(&patch)?;
    api.patch_status(&pod.name(), &PatchParams::default(), data)
        .await?;

    // Execute state.
    let transition = {
        state.next(Arc::clone(&provider_state), &pod).await?
    };

    // Handle transition
    match transition {
        Transition::Advance(s) => run_to_completion(client, s, provider_state, pod).await,
        Transition::Error(s) => run_to_completion(client, s, provider_state, pod).await,
        Transition::Complete(result) => result,
    }
}
