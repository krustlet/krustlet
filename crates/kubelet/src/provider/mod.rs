//! Traits and types needed to create backend providers for a Kubelet
use std::collections::HashMap;

use async_trait::async_trait;
use k8s_openapi::api::core::v1::{ConfigMap, EnvVarSource, Pod as KubePod, Secret};
use kube::api::{Api, WatchEvent};
use log::{error, info};
use thiserror::Error;

use crate::container::Container;
use crate::log::Sender;
use crate::node::Builder;
use crate::pod::Pod;

/// A back-end for a Kubelet.
///
/// The primary responsibility of a Provider is to execute a workload (or schedule it on an external executor)
/// and then monitor it, exposing details back upwards into the Kubelet.
///
/// We pass in the client to facilitate cases where a provider may be middleware for another Kubernetes object,
/// or where a provider may require supplemental Kubernetes objects such as Secrets, ConfigMaps, or CRDs.
///
/// **Note**: this trait is defined using [async-trait](https://crates.io/crates/async-trait) which
/// allows for the use of async methods on traits. The documentation reflects the generated code. It is
/// recommended for methods that return `Pin<Box<dyn Future<Output = Result<T>> + Send + 'async_trait>>`
/// to be implemented as async functions using `#[async_trait]`.
///
/// # Example
/// ```rust
/// use async_trait::async_trait;
/// use kubelet::pod::Pod;
/// use kubelet::provider::Provider;
///
/// struct MyProvider;
///
/// #[async_trait]
/// impl Provider for MyProvider {
///     const ARCH: &'static str = "my-arch";
///
///     async fn add(&self, pod: Pod) -> anyhow::Result<()> {
///         todo!("Implement Provider::add")
///     }
///
///     // Implement the rest of the methods using `async` for the ones that return futures ...
///     # async fn modify(&self, pod: Pod) -> anyhow::Result<()> { todo!() }
///     # async fn delete(&self, pod: Pod) -> anyhow::Result<()> { todo!() }
///     # async fn logs(&self, namespace: String, pod: String, container: String, sender: kubelet::log::Sender) -> anyhow::Result<()> { todo!() }
/// }
/// ```
#[async_trait]
pub trait Provider {
    /// Arch returns a string specifying what architecture this provider supports
    const ARCH: &'static str;

    /// Allows provider to populate node information.
    async fn node(&self, _builder: &mut Builder) -> anyhow::Result<()> {
        Ok(())
    }

    /// Given a Pod definition, execute the workload.
    async fn add(&self, pod: Pod) -> anyhow::Result<()>;

    /// Given an updated Pod definition, update the given workload.
    ///
    /// Pods that are sent to this function have already met certain criteria for modification.
    /// For example, updates to the `status` of a Pod will not be sent into this function.
    async fn modify(&self, pod: Pod) -> anyhow::Result<()>;

    /// Given the definition of a deleted Pod, remove the workload from the runtime.
    ///
    /// This does not need to actually delete the Pod definition -- just destroy the
    /// associated workload.
    async fn delete(&self, pod: Pod) -> anyhow::Result<()>;

    /// Given a Pod, get back the logs for the associated workload.
    async fn logs(
        &self,
        namespace: String,
        pod: String,
        container: String,
        sender: Sender,
    ) -> anyhow::Result<()>;

    /// Execute a given command on a workload and then return the result.
    ///
    /// The default implementation of this returns a message that this feature is
    /// not available. Override this only when there is an implementation.
    async fn exec(&self, _pod: Pod, _command: String) -> anyhow::Result<Vec<String>> {
        Err(NotImplementedError.into())
    }

    /// Determine what to do when a new event comes in.
    ///
    /// In most cases, this should not be overridden. It is exposed for rare cases when
    /// the underlying event handling needs to change.
    async fn handle_event(&self, event: WatchEvent<KubePod>) -> anyhow::Result<()> {
        match event {
            WatchEvent::Added(pod) => {
                let pod = pod.into();
                self.add(pod).await
            }
            WatchEvent::Modified(pod) => {
                let pod = pod.into();
                self.modify(pod).await
            }
            WatchEvent::Deleted(pod) => {
                let pod = pod.into();
                self.delete(pod).await
            }
            WatchEvent::Error(e) => {
                error!("Event error: {}", e);
                Err(e.into())
            }
            WatchEvent::Bookmark(_) => Ok(()),
        }
    }

    /// Resolve the environment variables for a container.
    ///
    /// This generally should not be overwritten unless you need to handle
    /// environment variable resolution in a special way, such as allowing
    /// custom Downward API fields.
    ///
    /// It is safe to call from within your own providers.
    async fn env_vars(
        container: &Container,
        pod: &Pod,
        client: &kube::Client,
    ) -> HashMap<String, String> {
        let mut env = HashMap::new();
        let vars = match container.env().as_ref() {
            Some(e) => e,
            None => return env,
        };

        for env_var in vars.clone().into_iter() {
            let key = env_var.name;
            let value = match env_var.value {
                Some(v) => v,
                None => {
                    on_missing_env_value(
                        env_var.value_from,
                        client,
                        pod.namespace(),
                        &field_map(pod),
                    )
                    .await
                }
            };
            env.insert(key, value);
        }
        env
    }
}

/// Called when an env var does not have a value associated with.
///
/// This follows the env_var_source to get the value
#[doc(hidden)]
async fn on_missing_env_value(
    env_var_source: Option<EnvVarSource>,
    client: &kube::Client,
    ns: &str,
    fields: &HashMap<String, String>,
) -> String {
    let env_src = match env_var_source {
        Some(env_src) => env_src,
        None => return String::new(),
    };

    // ConfigMaps
    if let Some(cfkey) = env_src.config_map_key_ref.as_ref() {
        let name = cfkey.name.as_deref().unwrap_or_default();
        match Api::<ConfigMap>::namespaced(client.clone(), ns)
            .get(name)
            .await
        {
            Ok(cfgmap) => {
                // I am not totally clear on what the outcome should
                // be of a cfgmap key miss. So for now just return an
                // empty default.
                return cfgmap
                    .data
                    .unwrap_or_default()
                    .get(&cfkey.key)
                    .cloned()
                    .unwrap_or_default();
            }
            Err(e) => {
                error!("Error fetching config map {}: {}", name, e);
                return "".to_string();
            }
        }
    }
    // Secrets
    if let Some(seckey) = env_src.secret_key_ref.as_ref() {
        let name = seckey.name.as_deref().unwrap_or_default();
        match Api::<Secret>::namespaced(client.clone(), ns)
            .get(name)
            .await
        {
            Ok(secret) => {
                // I am not totally clear on what the outcome should
                // be of a secret key miss. So for now just return an
                // empty default.
                return secret
                    .data
                    .unwrap_or_default()
                    .remove(&seckey.key)
                    .map(|s| String::from_utf8(s.0).unwrap_or_default())
                    .unwrap_or_default();
            }
            Err(e) => {
                error!("Error fetching secret {}: {}", name, e);
                return String::new();
            }
        }
    }
    // Downward API (Field Refs)
    if let Some(cfkey) = env_src.field_ref.as_ref() {
        return fields.get(&cfkey.field_path).cloned().unwrap_or_default();
    }
    // Reource Fields (Not implementable just yet... need more of a model.)

    String::new()
}

/// Build the map of allowable field_ref values.
///
/// The Downward API only supports a small selection of fields. This
/// provides those fields.
fn field_map(pod: &Pod) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    map.insert("metadata.name".into(), pod.name().to_owned());
    map.insert("metadata.namespace".into(), pod.namespace().to_owned());
    map.insert(
        "spec.serviceAccountName".into(),
        pod.service_account_name().unwrap_or_default().to_owned(),
    );
    map.insert(
        "status.hostIP".into(),
        pod.host_ip().unwrap_or_default().to_owned(),
    );
    map.insert(
        "status.podIP".into(),
        pod.pod_ip().unwrap_or_default().to_owned(),
    );
    pod.labels().iter().for_each(|(k, v)| {
        info!("adding {} to labels", k);
        map.insert(format!("metadata.labels.{}", k), v.clone());
    });
    pod.annotations().iter().for_each(|(k, v)| {
        map.insert(format!("metadata.annotations.{}", k), v.clone());
    });
    map
}

/// A Provider error
#[derive(Debug, Error)]
pub enum ProviderError {
    /// Pod was not found
    #[error("cannot find pod {}", pod_name)]
    PodNotFound {
        /// The pod's name
        pod_name: String,
    },
    /// Container was not found
    #[error("cannot find container {} in pod {}", container_name, pod_name)]
    ContainerNotFound {
        /// The container's pod's name
        pod_name: String,
        /// The container's name
        container_name: String,
    },
}

/// A specific operation is not implemented
#[derive(Error, Debug)]
#[error("Operation not supported")]
pub struct NotImplementedError;
