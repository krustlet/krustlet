use crate::Phase;
use k8s_openapi::api::core::v1::Container as KubeContainer;
use k8s_openapi::api::core::v1::Pod as KubePod;
use kube::{
    api::{Api, PatchParams},
    client::APIClient,
};
use log::{debug, error, info};

/// A Kubertnetes Pod
///
/// This is a new type around the k8s_openapi Pod definition
/// providing convenient accessor methods
#[derive(Default, Debug)]
pub struct Pod(KubePod);

impl Pod {
    /// Construct a new Pod
    pub fn new(inner: KubePod) -> Pod {
        Pod(inner)
    }

    /// Get the name of the pod
    pub fn name(&self) -> Option<&str> {
        self.0.metadata.as_ref()?.name.as_deref()
    }

    /// Get the pod's namespace
    ///
    /// Returns "default" if no namespace was explictily set
    pub fn namespace(&self) -> &str {
        let metadata = self.0.metadata.as_ref();
        metadata
            .and_then(|m| m.namespace.as_deref())
            .unwrap_or("default")
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
        self.0
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.labels.as_ref())
            .unwrap_or_else(|| &EMPTY_MAP)
    }

    ///  Get the pod's annotations
    pub fn annotations(&self) -> &std::collections::BTreeMap<String, String> {
        self.0
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.annotations.as_ref())
            .unwrap_or_else(|| &EMPTY_MAP)
    }

    ///  Get a specific annotation from the pod
    pub fn get_annotation(&self, key: &str) -> Option<&str> {
        Some(self.annotations().get(key)?.as_str())
    }

    /// Patch the pod status to update the phase.
    pub async fn patch_status(&self, client: APIClient, phase: &Phase) {
        let status = serde_json::json!(
            {
                "metadata": {
                    "resourceVersion": "",
                },
                "status": {
                    "phase": phase
                }
            }
        );

        let data = serde_json::to_vec(&status).expect("Should always serialize");
        let name = self.name().unwrap_or_default();
        let api: Api<KubePod> = Api::namespaced(client, self.namespace());
        match api.patch_status(&name, &PatchParams::default(), data).await {
            Ok(o) => {
                info!("Pod status for {} set to {:?}", name, phase);
                debug!("Pod status returned: {:#?}", o.status)
            }
            Err(e) => error!("Pod status update failed for {}: {}", name, e),
        }
    }

    /// Get a pod's containers
    pub fn containers(&self) -> &Vec<KubeContainer> {
        self.0
            .spec
            .as_ref()
            .map(|s| &s.containers)
            .unwrap_or_else(|| &EMPTY_VEC)
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
