//! Used to define a state machine of Pod states.
use log::info;

// pub mod default;
#[macro_use]
pub mod macros;

use crate::pod::Pod;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::{Api, PatchParams};

/// Tokio Receiver of PodChange events.
pub type PodChangeRx = tokio::sync::mpsc::Receiver<crate::pod::PodChange>;

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
pub trait State<PodState>: Sync + Send + 'static + std::fmt::Debug {
    /// The next state on success.
    type Success: State<PodState>;
    /// The next state on error.
    type Error: State<PodState>;

    /// Provider supplies method to be executed when in this state.
    async fn next(
        self,
        pod_state: &mut PodState,
        pod: &Pod,
        state_rx: &mut PodChangeRx,
    ) -> anyhow::Result<Transition<Self::Success, Self::Error>>;

    /// Provider supplies JSON status patch to apply when entering this state.
    async fn json_status(
        &self,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> anyhow::Result<serde_json::Value>;
}

#[async_recursion::async_recursion]
/// Recursively evaluate state machine until a state returns Complete.
pub async fn run_to_completion<PodState: Send + Sync + 'static>(
    client: kube::Client,
    state: impl State<PodState>,
    mut pod_state: PodState,
    pod: Pod,
    mut state_rx: PodChangeRx,
) -> anyhow::Result<()> {
    info!("Pod {} entering state {:?}", pod.name(), state);

    // When handling a new state, we update the Pod state with Kubernetes.
    let api: Api<KubePod> = Api::namespaced(client.clone(), pod.namespace());
    let patch = state.json_status(&mut pod_state, &pod).await?;
    info!("Pod {} status patch: {:?}", pod.name(), &patch);
    let data = serde_json::to_vec(&patch)?;
    api.patch_status(&pod.name(), &PatchParams::default(), data)
        .await?;

    info!("Pod {} executing state handler {:?}", pod.name(), state);
    // Execute state.
    let transition = { state.next(&mut pod_state, &pod, &mut state_rx).await? };

    info!(
        "Pod {} state execution result: {:?}",
        pod.name(),
        transition
    );

    // Handle transition
    match transition {
        Transition::Advance(s) => run_to_completion(client, s, pod_state, pod, state_rx).await,
        Transition::Error(s) => run_to_completion(client, s, pod_state, pod, state_rx).await,
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
        _pod_state: &mut P,
        _pod: &Pod,
        _state_rx: &mut PodChangeRx,
    ) -> anyhow::Result<Transition<Self::Success, Self::Error>> {
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
