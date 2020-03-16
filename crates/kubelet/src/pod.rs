use k8s_openapi::api::core::v1 as V1Api;
use kube::{
    api::{Api, PatchParams},
    client::APIClient,
};
use log::{debug, error, info};

pub struct Pod(V1Api::Pod);
pub type ApiPod = V1Api::Pod;

impl std::convert::From<ApiPod> for Pod {
    fn from(api_pod: ApiPod) -> Self {
        Self(api_pod)
    }
}

impl<'a> std::convert::From<&'a Pod> for &'a ApiPod {
    fn from(pod: &'a Pod) -> Self {
        &pod.0
    }
}

impl Pod {
    pub fn as_serialized(&self) -> &ApiPod {
        self.into()
    }

    /// Get the name of the pod
    pub fn name(&self) -> Option<&str> {
        self.0.metadata.as_ref()?.name.as_deref()
    }

    pub fn namespace(&self) -> &str {
        let metadata = self.0.metadata.as_ref();
        metadata
            .and_then(|m| m.namespace.as_deref())
            .unwrap_or("default")
    }

    /// Get the name of the pod
    pub fn node_selector(&self) -> Option<&std::collections::BTreeMap<String, String>> {
        self.0.spec.as_ref()?.node_selector.as_ref()
    }

    pub fn service_account_name(&self) -> Option<&str> {
        let spec = self.0.spec.as_ref()?;
        spec.service_account_name.as_deref()
    }

    pub fn host_ip(&self) -> Option<&str> {
        let status = self.0.status.as_ref()?;
        status.host_ip.as_deref()
    }

    pub fn pod_ip(&self) -> Option<&str> {
        let status = self.0.status.as_ref()?;
        status.pod_ip.as_deref()
    }

    pub fn labels_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.labels()
            .map(|a| Either::A(a.iter()))
            .unwrap_or_else(|| Either::B(std::iter::empty()))
    }

    pub fn annotations_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.annotations()
            .map(|a| Either::A(a.iter()))
            .unwrap_or_else(|| Either::B(std::iter::empty()))
    }

    pub fn get_annotation(&self, key: &str) -> Option<&str> {
        Some(self.annotations()?.get(key)?.as_str())
    }

    /// Patch the pod status to update the phase.
    pub async fn patch_status(&self, client: APIClient, phase: &str, ns: &str) {
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
        let api: Api<ApiPod> = Api::namespaced(client, ns);
        match api.patch_status(&name, &PatchParams::default(), data).await {
            Ok(o) => {
                info!("Pod status for {} set to {}", name, phase);
                debug!("Pod status returned: {:#?}", o.status)
            }
            Err(e) => error!("Pod status update failed for {}: {}", name, e),
        }
    }

    pub fn annotations(&self) -> Option<&std::collections::BTreeMap<String, String>> {
        let metadata = self.0.metadata.as_ref()?;
        Some(metadata.annotations.as_ref()?)
    }

    pub fn labels(&self) -> Option<&std::collections::BTreeMap<String, String>> {
        let metadata = self.0.metadata.as_ref()?;
        Some(metadata.labels.as_ref()?)
    }
}

enum Either<T, U> {
    A(T),
    B(U),
}

impl<T, U, V> Iterator for Either<T, U>
where
    T: Iterator<Item = V>,
    U: Iterator<Item = V>,
{
    type Item = V;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Either::A(i) => i.next(),
            Either::B(i) => i.next(),
        }
    }
}
