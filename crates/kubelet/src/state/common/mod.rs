//! Contains a set of generic states where there is little provider-specific
//! logic, and the machinery to use them. This removes the need to write these
//! states in many providers; instead, the provider need only implement the
//! GenericProviderState and GenericPodState traits for its state types.

use crate::pod::state::prelude::PodStatus;
use crate::pod::Pod;
use crate::provider::{DevicePluginSupport, PluginSupport, VolumeSupport};
use krator::{ObjectState, State};
use std::collections::HashMap;

pub mod crash_loop_backoff;
pub mod error;
pub mod image_pull;
pub mod image_pull_backoff;
pub mod registered;
pub mod resources;
pub mod terminated;
pub mod volume_mount;

/// Types of error condition whose backoff should be tracked independently.
pub enum BackoffSequence {
    /// Backoff from a failed image pull.
    ImagePull,
    /// Backoff from a pod crash.
    CrashLoop,
}

/// Indicates whether a threshold has been triggered.
pub enum ThresholdTrigger {
    /// The threshold has been triggered.
    Triggered,
    /// The threshold has not been triggered.
    Untriggered,
}

/// Exposes provider-wide state in a way that can be consumed by
/// the generic states.
#[async_trait::async_trait]
pub trait GenericProviderState: 'static + Send + Sync {
    /// Gets a Kubernetes client. This is a provider function to enable the
    /// provider to control the client configuration as desired.
    fn client(&self) -> kube::Client;
    /// Gets the `Store` used by the provider.
    fn store(&self) -> std::sync::Arc<dyn crate::store::Store + Sync + Send>;
    /// Stops the specified pod. This typically involves tearing down a
    /// runtime or other execution environment.
    async fn stop(&self, pod: &crate::pod::Pod) -> anyhow::Result<()>;
}

/// Exposes pod state in a way that can be consumed by
/// the generic states.
#[async_trait::async_trait]
pub trait GenericPodState: ObjectState<Manifest = Pod, Status = PodStatus> {
    /// Stores the environment variables that are added through state conditions
    /// rather than being from PodSpecs.
    async fn set_env_vars(&mut self, env_vars: HashMap<String, HashMap<String, String>>);
    /// Stores the pod module binaries for future execution. Typically your
    /// implementation can just move the modules map into a member field.
    async fn set_modules(&mut self, modules: HashMap<String, Vec<u8>>);
    /// Stores the pod volume references for future mounting into
    /// the provider's execution environment. Typically your
    /// implementation can just move the volumes map into a member field.
    async fn set_volumes(&mut self, volumes: HashMap<String, crate::volume::VolumeRef>);
    /// Backs off (waits) after an error of the specified kind.
    async fn backoff(&mut self, sequence: BackoffSequence);
    /// Resets the backoff time for the specified kind of error.
    async fn reset_backoff(&mut self, sequence: BackoffSequence);
    /// Increments an error count and returns whether the number of errors
    /// has passed the provider's threshold for entering CrashLoopBackoff.
    async fn record_error(&mut self) -> ThresholdTrigger;
}

/// A provider that wants to use the generic states implemented in this
/// module.
pub trait GenericProvider: 'static + Send + Sync {
    /// The state of the provider itself.
    type ProviderState: GenericProviderState + VolumeSupport + PluginSupport + DevicePluginSupport;
    /// The state that is passed between Pod state handlers.
    type PodState: GenericPodState + ObjectState<SharedState = Self::ProviderState>;
    /// The state to which pods should transition after they have completed
    /// all generic states. Typically this is the state which first runs
    /// any pod binary (for example, the state which runs init containers).
    type RunState: Default + State<Self::PodState>;

    /// Validates that the pod specification is compatible with the provider.
    /// If not, implementations should return an Err value with
    /// a description of why the pod cannot be run.
    ///
    /// Implementations do not need validate individual containers; this is
    /// done in `validate_container_runnable`.
    fn validate_pod_runnable(pod: &crate::pod::Pod) -> anyhow::Result<()>;

    /// Validates that the container specification is compatible with the provider.
    /// If not, implementations should return an Err value with
    /// a description of why the pod cannot be run.
    fn validate_container_runnable(container: &crate::container::Container) -> anyhow::Result<()>;

    /// Validates that the pod specification, including all containers, is
    /// compatible with the provider. The default implementation calls
    /// `validate_pod_runnable`, then `validate_container_runnable` for each
    /// container.
    fn validate_pod_and_containers_runnable(pod: &crate::pod::Pod) -> anyhow::Result<()> {
        Self::validate_pod_runnable(pod)?;
        for container in pod.containers() {
            Self::validate_container_runnable(&container)?;
        }
        Ok(())
    }
}
