use std::collections::HashMap;

use log::info;

use super::starting::Starting;
use crate::states::container::ContainerHandleMap;
use crate::{PodState, ProviderState};
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::api::Api;
use kubelet::backoff::BackoffStrategy;
use kubelet::pod::state::prelude::*;
use kubelet::state::common::error::Error;
use kubelet::state::common::GenericProviderState;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(Starting)]
pub struct Initializing;

#[async_trait::async_trait]
impl State<PodState> for Initializing {
    async fn next(
        self: Box<Self>,
        provider_state: SharedState<ProviderState>,
        pod_state: &mut PodState,
        pod: &Pod,
    ) -> Transition<PodState> {
        let _client: Api<KubePod> =
            Api::namespaced(provider_state.read().await.client(), pod.namespace());
        let container_handles: ContainerHandleMap = HashMap::new();
        // TODO: Run Container State Machines
        for _init_container in pod.init_containers() {}
        info!("Finished init containers for pod {:?}", pod.name());
        pod_state.crash_loop_backoff_strategy.reset();
        Transition::next(self, Starting::new(container_handles))
    }

    async fn status(&self, _pod_state: &mut PodState, _pmeod: &Pod) -> anyhow::Result<PodStatus> {
        Ok(make_status(Phase::Running, "Initializing"))
    }
}

impl TransitionTo<Error<crate::WasiProvider>> for Initializing {}
