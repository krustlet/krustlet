use super::DeviceIdMap;
use crate::device_plugin_api::v1beta1::ContainerAllocateResponse;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, ListParams};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// `DeviceAllocateInfo` contains the device ids reserved to a container for a specific resource and
/// the ContainerAllocateResponse which contains information about what to mount into the container
#[derive(Clone, Debug)]
pub struct DeviceAllocateInfo {
    /// Contains the device Ids allocated to this container for the given resource name
    pub device_ids: HashSet<String>,
    /// Contains the RPC ContainerAllocateResponse for the device_ids
    pub allocate_response: ContainerAllocateResponse,
}
/// Map of devices allocated to the container, keyed by resource name
type ResourceAllocateInfo = HashMap<String, DeviceAllocateInfo>;
/// Map of container device information, keyed by container name
pub type ContainerDevices = HashMap<String, ResourceAllocateInfo>;

/// PodDevices contains the map of Pods to allocated devices This is a very nested structure modeled
/// after Kubernetes (see
/// https://github.com/kubernetes/kubernetes/blob/master/pkg/kubelet/cm/devicemanager/pod_devices.go).
/// This could potentially be simplified by use of an in memory database.
#[derive(Clone)]
pub struct PodDevices {
    /// Name of the node this kubelet is running on
    node_name: String,
    /// Map of devices allocated to the Pod keyed by Pod UID
    allocated_devices: Arc<Mutex<HashMap<String, ContainerDevices>>>,
    /// Kubernetes API client for making Node status patches
    client: kube::Client,
}

impl PodDevices {
    pub fn new(node_name: &str, client: kube::Client) -> Self {
        PodDevices {
            node_name: node_name.to_string(),
            allocated_devices: Arc::new(Mutex::new(HashMap::new())),
            client,
        }
    }

    /// A method for listing active pods on the node so the amount of device plugin resources
    /// requested by existing pods could be counted when updating allocated devices
    pub async fn get_active_pods(&self) -> anyhow::Result<HashSet<String>> {
        let pod_client: Api<Pod> = Api::all(self.client.clone());
        let pods = pod_client
            .list(&ListParams::default().fields(&format!("spec.nodeName={}", self.node_name)))
            .await?;
        Ok(pods
            .iter()
            .map(|pod| {
                pod.metadata
                    .uid
                    .clone()
                    .expect("Pod uid should always be set but was not")
            })
            .collect::<HashSet<String>>())
    }

    /// get_pods returns the UIDs of all the Pods in the `PodDevices` map
    pub fn get_pods(&self) -> HashSet<String> {
        self.allocated_devices
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    pub fn remove_pods(&self, pods_to_remove: HashSet<String>) -> anyhow::Result<()> {
        let mut allocated_devices = self.allocated_devices.lock().unwrap();
        pods_to_remove.iter().for_each(|p_uid| {
            allocated_devices.remove(p_uid);
        });
        Ok(())
    }

    /// Returns the device Ids of all devices requested by a container
    pub fn get_container_devices(
        &self,
        resource_name: &str,
        pod_uid: &str,
        container_name: &str,
    ) -> Option<HashSet<String>> {
        let allocated_devices = self.allocated_devices.lock().unwrap().clone();
        if let Some(container_devices) = allocated_devices.get(pod_uid) {
            if let Some(resource_devices) = container_devices.get(container_name) {
                if let Some(device_allocate_info) = resource_devices.get(resource_name) {
                    return Some(device_allocate_info.device_ids.clone());
                }
            }
        }
        None
    }

    pub fn get_allocated_devices(&self) -> DeviceIdMap {
        let allocated_devices = self.allocated_devices.lock().unwrap().clone();
        let mut res: DeviceIdMap = HashMap::new();
        allocated_devices
            .iter()
            .for_each(|(_p_uid, container_map)| {
                container_map
                    .iter()
                    .for_each(|(_container_name, container_devs)| {
                        container_devs
                            .iter()
                            .for_each(|(resource_name, device_allocate_info)| {
                                if let Some(device_ids) = res.get_mut(resource_name) {
                                    device_allocate_info.device_ids.iter().for_each(|id| {
                                        device_ids.insert(id.clone());
                                    });
                                } else {
                                    res.insert(
                                        resource_name.clone(),
                                        device_allocate_info.device_ids.clone(),
                                    );
                                }
                            });
                    });
            });
        res
    }

    pub fn add_allocated_devices(&self, pod_uid: &str, container_devices: ContainerDevices) {
        self.allocated_devices
            .lock()
            .unwrap()
            .entry(pod_uid.to_string())
            .and_modify(|m| m.extend(container_devices.clone()))
            .or_insert(container_devices);
    }

    /// Returns all of the allocate responses for a Pod. Used to set mounts, env vars, annotations,
    /// and device specs for Pod.
    pub fn get_pod_allocate_responses(
        &self,
        pod_uid: &str,
    ) -> Option<HashMap<String, Vec<ContainerAllocateResponse>>> {
        match self.allocated_devices.lock().unwrap().get(pod_uid) {
            Some(container_devices) => {
                let mut container_allocate_responses = HashMap::new();
                container_devices
                    .iter()
                    .for_each(|(container_name, resource_allocate_info)| {
                        container_allocate_responses.insert(
                            container_name.clone(),
                            resource_allocate_info
                                .values()
                                .cloned()
                                .map(|dev_info| dev_info.allocate_response)
                                .collect(),
                        );
                    });
                Some(container_allocate_responses)
            }
            None => None,
        }
    }
}
