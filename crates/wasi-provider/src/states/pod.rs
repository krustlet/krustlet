use crate::ModuleRunContext;
use crate::SharedPodState;
use kubelet::backoff::ExponentialBackoffStrategy;
use kubelet::pod::Pod;
use kubelet::pod::PodKey;
use kubelet::pod::Status;
use kubelet::state::ResourceState;
use tokio::sync::mpsc;

pub(crate) mod completed;
pub(crate) mod crash_loop_backoff;
pub(crate) mod error;
pub(crate) mod image_pull;
pub(crate) mod image_pull_backoff;
pub(crate) mod initializing;
pub(crate) mod registered;
pub(crate) mod running;
pub(crate) mod starting;
pub(crate) mod terminated;
pub(crate) mod volume_mount;
pub(crate) mod wont_run;


/// State that is shared between pod state handlers.
pub struct PodState {
    key: PodKey,
    run_context: ModuleRunContext,
    errors: usize,
    image_pull_backoff_strategy: ExponentialBackoffStrategy,
    crash_loop_backoff_strategy: ExponentialBackoffStrategy,
}
impl ResourceState for PodState {
    type Manifest = Pod;
    type Status = Status;
}
// No cleanup state needed, we clean up when dropping PodState.
#[async_trait]
impl kubelet::state::AsyncDrop for PodState {
    type ProviderState = ProviderState;
    async fn async_drop(self, provider_state: &mut ProviderState) {
        {
            let mut handles = provider_state.handles.write().await;
            handles.remove(&self.key);
        }
    }
}

impl PodState {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel(pod.all_containers().len());
        let run_context = ModuleRunContext {
            modules: Default::default(),
            volumes: Default::default(),
            status_sender: tx,
            status_recv: rx,
        };
        let key = PodKey::from(pod);
        PodState {
            key,
            run_context,
            errors: 0,
            image_pull_backoff_strategy: ExponentialBackoffStrategy::default(),
            crash_loop_backoff_strategy: ExponentialBackoffStrategy::default(),
        }
    }
}

#[async_trait]
impl GenericPodState for PodState {
    fn set_modules(&mut self, modules: HashMap<String, Vec<u8>>) {
        self.run_context.modules = modules;
    }
    fn set_volumes(&mut self, volumes: HashMap<String, kubelet::volume::Ref>) {
        self.run_context.volumes = volumes;
    }
    async fn backoff(&mut self, sequence: BackoffSequence) {
        let backoff_strategy = match sequence {
            BackoffSequence::ImagePull => &mut self.image_pull_backoff_strategy,
            BackoffSequence::CrashLoop => &mut self.crash_loop_backoff_strategy,
        };
        backoff_strategy.wait().await;
    }
    fn reset_backoff(&mut self, sequence: BackoffSequence) {
        let backoff_strategy = match sequence {
            BackoffSequence::ImagePull => &mut self.image_pull_backoff_strategy,
            BackoffSequence::CrashLoop => &mut self.crash_loop_backoff_strategy,
        };
        backoff_strategy.reset();
    }
    fn record_error(&mut self) -> ThresholdTrigger {
        self.errors += 1;
        if self.errors > 3 {
            self.errors = 0;
            ThresholdTrigger::Triggered
        } else {
            ThresholdTrigger::Untriggered
        }
    }
}
