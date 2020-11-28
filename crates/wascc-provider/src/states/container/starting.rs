use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::TryFrom;
use std::ops::Deref;
use std::sync::Arc;

use log::{debug, error, info};
use tokio::sync::Mutex;

use kubelet::container::{Container, ContainerKey, Handle as ContainerHandle};
use kubelet::pod::state::prelude::*;
use kubelet::pod::{Handle, PodKey};
use kubelet::provider::Provider;

use crate::rand::Rng;
use crate::PodState;
use crate::VolumeBinding;
use crate::{
    fail_fatal, transition_to_error, wascc_run, ActorHandle, LogHandleFactory, WasccProvider,
};

/// The container is starting.
#[derive(Default, Debug, TransitionTo)]
#[transition_to()]
pub struct Starting;

#[async_trait::async_trait]
impl State<ContainerState, ContainerStatus> for Starting {
    async fn next(self: Box<Self>, state: &mut PodState, pod: &Pod) -> Transition<PodState> {
        todo!()
    }

    async fn status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<PodStatus> {
        todo!()
    }
}
