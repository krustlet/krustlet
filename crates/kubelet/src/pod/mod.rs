//! `pod` is a collection of utilities surrounding the Kubernetes pod API.
mod handle;
mod queue;
mod status;
pub use handle::{key_from_pod, pod_key, Handle};
pub use queue::PodChange;
pub(crate) use queue::Queue;
pub use status::{update_status, Phase, Status, StatusMessage};

use crate::container::{Container, ContainerKey};
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::{
    Container as KubeContainer, Pod as KubePod, Volume as KubeVolume,
};
use kube::api::Meta;

/// A Kubernetes Pod
///
/// This is a new type around the k8s_openapi Pod definition
/// providing convenient accessor methods
#[derive(Default, Debug, Clone)]
pub struct Pod(KubePod);

impl Pod {
    /// Construct a new Pod
    pub fn new(inner: KubePod) -> Self {
        Self(inner)
    }

    /// Get the name of the pod
    pub fn name(&self) -> &str {
        self.0
            .metadata
            .name
            .as_deref()
            .expect("Pod name should always be set but was not")
    }

    /// Get the pod's namespace
    ///
    /// Returns "default" if no namespace was explictily set
    pub fn namespace(&self) -> &str {
        self.0.metadata.namespace.as_deref().unwrap_or("default")
    }

    /// Get the pod's node_selector map
    pub fn node_selector(&self) -> Option<&std::collections::BTreeMap<String, String>> {
        self.0.spec.as_ref()?.node_selector.as_ref()
    }

    /// Get the pod's service account name
    pub fn service_account_name(&self) -> Option<&str> {
        let spec = self.0.spec.as_ref()?;
        spec.service_account_name.as_deref()
    }

    /// Get the pod volumes
    pub fn volumes(&self) -> Option<&Vec<KubeVolume>> {
        let spec = self.0.spec.as_ref()?;
        spec.volumes.as_ref()
    }

    /// Get the pod's host ip
    pub fn host_ip(&self) -> Option<&str> {
        let status = self.0.status.as_ref()?;
        status.host_ip.as_deref()
    }

    /// Get the pod's ip
    pub fn pod_ip(&self) -> Option<&str> {
        let status = self.0.status.as_ref()?;
        status.pod_ip.as_deref()
    }

    /// Get an iterator over the pod's labels
    pub fn labels(&self) -> &std::collections::BTreeMap<String, String> {
        self.0.meta().labels.as_ref().unwrap_or_else(|| &EMPTY_MAP)
    }

    ///  Get the pod's annotations
    pub fn annotations(&self) -> &std::collections::BTreeMap<String, String> {
        self.0
            .meta()
            .annotations
            .as_ref()
            .unwrap_or_else(|| &EMPTY_MAP)
    }

    /// Get the names of the pod's image pull secrets
    pub fn image_pull_secrets(&self) -> Vec<String> {
        match self.0.spec.as_ref() {
            None => vec![],
            Some(spec) => match spec.image_pull_secrets.as_ref() {
                None => vec![],
                Some(objrefs) => objrefs
                    .iter()
                    .filter_map(|objref| objref.name.clone())
                    .collect(),
            },
        }
    }

    /// Indicate if this pod is a static pod.
    /// TODO: A missing owner_references field was an indication of static pod in my testing but I
    /// dont know how reliable this is.
    pub fn is_static(&self) -> bool {
        self.0.meta().owner_references.is_none()
    }

    /// Indicate if this pod is part of a Daemonset
    pub fn is_daemonset(&self) -> bool {
        if let Some(owners) = &self.0.meta().owner_references {
            for owner in owners {
                if owner.kind == "DaemonSet" {
                    return true;
                }
            }
        }
        false
    }

    ///  Get a specific annotation from the pod
    pub fn get_annotation(&self, key: &str) -> Option<&str> {
        Some(self.annotations().get(key)?.as_str())
    }

    /// Get the deletionTimestamp if it exists
    pub fn deletion_timestamp(&self) -> Option<&DateTime<Utc>> {
        self.0.meta().deletion_timestamp.as_ref().map(|t| &t.0)
    }

    /// Get a pod's containers
    pub fn containers(&self) -> Vec<Container> {
        self.0
            .spec
            .as_ref()
            .map(|s| &s.containers)
            .unwrap_or_else(|| &EMPTY_VEC)
            .iter()
            .map(|c| Container::new(c))
            .collect()
    }

    /// Get a pod's init containers
    pub fn init_containers(&self) -> Vec<Container> {
        self.0
            .spec
            .as_ref()
            .and_then(|s| s.init_containers.as_ref())
            .unwrap_or(&EMPTY_VEC)
            .iter()
            .map(|c| Container::new(c))
            .collect()
    }

    /// Gets all of a pod's containers (init and application)
    pub fn all_containers(&self) -> Vec<ContainerKey> {
        let app_containers = self.containers();
        let app_container_keys = app_containers
            .iter()
            .map(|c| ContainerKey::App(c.name().to_owned()));
        let init_containers = self.containers();
        let init_container_keys = init_containers
            .iter()
            .map(|c| ContainerKey::Init(c.name().to_owned()));
        app_container_keys.chain(init_container_keys).collect()
    }

    /// Turn the Pod into the Kubernetes API version of a Pod
    pub fn into_kube_pod(self) -> KubePod {
        self.0
    }

    /// Turn a reference to a Pod into a reference to the Kubernetes API version of a Pod
    pub fn as_kube_pod(&self) -> &KubePod {
        &self.0
    }
}

impl std::convert::From<KubePod> for Pod {
    fn from(api_pod: KubePod) -> Self {
        Self(api_pod)
    }
}

impl<'a> std::convert::From<&'a Pod> for &'a KubePod {
    fn from(pod: &'a Pod) -> Self {
        &pod.0
    }
}
impl std::convert::From<Pod> for KubePod {
    fn from(pod: Pod) -> Self {
        pod.0
    }
}

lazy_static::lazy_static! {
    static ref EMPTY_MAP: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    static ref EMPTY_VEC: Vec<KubeContainer> = Vec::new();
}
