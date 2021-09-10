use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use krator::{ObjectState, SharedState};
use kubelet::backoff::BackoffStrategy;
use kubelet::backoff::ExponentialBackoffStrategy;
use kubelet::pod::Pod;
use kubelet::pod::PodKey;
use kubelet::pod::Status;
use kubelet::state::common::{BackoffSequence, GenericPodState, ThresholdTrigger};
use tokio::sync::RwLock;
use tracing::error;

use crate::ModuleRunContext;
use crate::ProviderState;

pub(crate) mod completed;
pub(crate) mod initializing;
pub(crate) mod running;
pub(crate) mod starting;

/// State that is shared between pod state handlers.
pub struct PodState {
    key: PodKey,
    run_context: SharedState<ModuleRunContext>,
    errors: usize,
    image_pull_backoff_strategy: ExponentialBackoffStrategy,
    pub(crate) crash_loop_backoff_strategy: ExponentialBackoffStrategy,
}

#[async_trait]
impl ObjectState for PodState {
    type Manifest = Pod;
    type Status = Status;
    type SharedState = ProviderState;
    async fn async_drop(self, provider_state: &mut Self::SharedState) {
        {
            {
                let mut context = self.run_context.write().await;
                let unmounts = context.volumes.iter_mut().map(|(k, vol)| async move {
                    if let Err(e) = vol.unmount().await {
                        // Just log the error, as there isn't much we can do here
                        error!(error = %e, volume_name = %k, "Unable to unmount volume");
                    }
                });
                futures::future::join_all(unmounts).await;
            }
            let mut handles = provider_state.handles.write().await;
            handles.remove(&self.key);
        }
    }
}

impl PodState {
    pub fn new(pod: &Pod) -> Self {
        let run_context = ModuleRunContext {
            modules: Default::default(),
            volumes: Default::default(),
            env_vars: Default::default(),
        };
        let key = PodKey::from(pod);
        PodState {
            key,
            run_context: Arc::new(RwLock::new(run_context)),
            errors: 0,
            image_pull_backoff_strategy: ExponentialBackoffStrategy::default(),
            crash_loop_backoff_strategy: ExponentialBackoffStrategy::default(),
        }
    }
}

#[async_trait]
impl GenericPodState for PodState {
    async fn set_env_vars(&mut self, env_vars: HashMap<String, HashMap<String, String>>) {
        let mut run_context = self.run_context.write().await;
        run_context.env_vars = env_vars;
    }
    async fn set_modules(&mut self, modules: HashMap<String, Vec<u8>>) {
        let mut run_context = self.run_context.write().await;
        run_context.modules = modules;
    }
    // For this provider, set_volumes extends the current volumes rather than re-assigning
    async fn set_volumes(&mut self, volumes: HashMap<String, kubelet::volume::VolumeRef>) {
        let mut run_context = self.run_context.write().await;
        run_context.volumes.extend(volumes);
    }
    async fn backoff(&mut self, sequence: BackoffSequence) {
        let backoff_strategy = match sequence {
            BackoffSequence::ImagePull => &mut self.image_pull_backoff_strategy,
            BackoffSequence::CrashLoop => &mut self.crash_loop_backoff_strategy,
        };
        backoff_strategy.wait().await;
    }
    async fn reset_backoff(&mut self, sequence: BackoffSequence) {
        let backoff_strategy = match sequence {
            BackoffSequence::ImagePull => &mut self.image_pull_backoff_strategy,
            BackoffSequence::CrashLoop => &mut self.crash_loop_backoff_strategy,
        };
        backoff_strategy.reset();
    }
    async fn record_error(&mut self) -> ThresholdTrigger {
        self.errors += 1;
        if self.errors > 3 {
            self.errors = 0;
            ThresholdTrigger::Triggered
        } else {
            ThresholdTrigger::Untriggered
        }
    }
}
