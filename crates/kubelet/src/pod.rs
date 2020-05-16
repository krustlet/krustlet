use std::collections::HashMap;

use crate::status::{Phase, Status};
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::{
    Container as KubeContainer, ContainerStatus as KubeContainerStatus, Pod as KubePod,
    Volume as KubeVolume,
};
use kube::api::{Api, Meta, PatchParams};
use log::{debug, error};

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
            .as_ref()
            .and_then(|m| m.name.as_deref())
            .expect("Pod name should always be set but was not")
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
                match owner.kind.as_ref() {
                    "DaemonSet" => return true,
                    _ => (),
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

    /// Patch the pod status using the given status information.
    pub async fn patch_status(&self, client: kube::Client, status: Status) {
        let name = self.name();
        let api: Api<KubePod> = Api::namespaced(client, self.namespace());
        let current_status = match api.get(name).await {
            Ok(p) => match p.status {
                Some(s) => s,
                None => {
                    error!("Pod is missing status information. This should not occur");
                    return;
                }
            },
            Err(e) => {
                error!("Unable to fetch current status of pod {}, aborting status patch (will be retried on next status update): {:?}", name, e);
                return;
            }
        };

        // This section figures out what the current phase of the pod should be
        // based on the container statuses
        let current_statuses = status
            .container_statuses
            .into_iter()
            .map(|s| (s.0.clone(), s.1.to_kubernetes(s.0)))
            .collect::<HashMap<String, KubeContainerStatus>>();
        // Filter out any ones we are updating and then combine them all together
        let mut container_statuses = current_status
            .container_statuses
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !current_statuses.contains_key(&s.name))
            .collect::<Vec<KubeContainerStatus>>();
        container_statuses.extend(current_statuses.into_iter().map(|(_, v)| v));
        let mut num_succeeded: usize = 0;
        let mut failed = false;
        // TODO(thomastaylor312): Add inferring a message from these container
        // statuses if there is no message passed in the Status object
        for status in container_statuses.iter() {
            // Basically anything is considered running phase in kubernetes
            // unless it is explicitly exited, so don't worry about considering
            // that state. We only really need to check terminated
            if let Some(terminated) = &status.state.as_ref().unwrap().terminated {
                if terminated.exit_code != 0 {
                    failed = true;
                    break;
                } else {
                    num_succeeded += 1
                }
            }
        }
        // is there ever a case when we get a status that we should end up in Phase unknown?
        let phase = if num_succeeded == container_statuses.len() {
            Phase::Succeeded
        } else if failed {
            Phase::Failed
        } else {
            Phase::Running
        };

        let json_status = serde_json::json!(
            {
                "metadata": {
                    "resourceVersion": "",
                },
                "status": {
                    "phase": phase,
                    "message": status.message,
                    "containerStatuses": container_statuses
                }
            }
        );

        debug!("Setting pod status for {} using {:?}", name, json_status);

        let data = serde_json::to_vec(&json_status).expect("Should always serialize");
        match api.patch_status(&name, &PatchParams::default(), data).await {
            Ok(o) => debug!("Pod status returned: {:#?}", o.status),
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
