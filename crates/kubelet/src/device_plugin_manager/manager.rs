//! The Kubelet device plugin manager. Consists of a `DeviceRegistry` that hosts a registration service for device plugins, a `DeviceManager` that maintains a device plugin client for each registered device plugin, a `NodePatcher` that patches the Node status with the extended resources advertised by device plugins, and a `PodDevices` that maintains a list of Pods that are actively using allocated resources.
use crate::device_plugin_api::v1beta1::{
    device_plugin_client::DevicePluginClient, AllocateRequest, ContainerAllocateRequest,
    ContainerAllocateResponse, RegisterRequest, API_VERSION,
};
use crate::grpc_sock;
use crate::pod::Pod;

use super::node_patcher::NodeStatusPatcher;
use super::plugin_connection::PluginConnection;
use super::pod_devices::{ContainerDevices, DeviceAllocateInfo, PodDevices};
use super::{DeviceIdMap, DeviceMap, PluginDevicesMap, PodResourceRequests, HEALTHY};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;
#[cfg(target_family = "windows")]
use tokio_compat_02::FutureExt;
use tracing::trace;

#[cfg(target_family = "unix")]
const DEFAULT_PLUGIN_PATH: &str = "/var/lib/kubelet/device_plugins/";
#[cfg(target_family = "windows")]
const DEFAULT_PLUGIN_PATH: &str = "c:\\ProgramData\\kubelet\\device_plugins";

const UPDATE_NODE_STATUS_CHANNEL_SIZE: usize = 15;

/// ContainerAllocateInfo pairs an allocate request to with the requesting container
#[derive(Clone)]
pub struct ContainerAllocateInfo {
    /// The name of the container
    container_name: String,
    /// The `ContainerAllocateRequest` sent to the device plugin for this container
    container_allocate_request: ContainerAllocateRequest,
}

/// An implementation of the Kubernetes Device Plugin Manager (https://github.com/kubernetes/kubernetes/tree/v1.21.1/pkg/kubelet/cm/devicemanager).
/// It implements the device plugin framework's `Registration` gRPC service. A device plugin (DP) can register itself with the kubelet through this gRPC
/// service. This allows the DP to advertise a resource like system hardware kubelet. The `DeviceManager` contains a `NodePatcher` that patches the Node
/// with resources advertised by DPs. Then the K8s scheduler can schedule Pods that request those resources to this Node. Once scheduled, the device manager
/// confirms notifies the DP that it wants to use on of its resources by making an `allocate` gRPC call. On success, it ensures that all necessary mounts,
/// environment variables, annotations, and device specs are added to the Pod (which's Containers) are requesting the DP resource.
#[derive(Clone)]
pub struct DeviceManager {
    /// Map of registered device plugins, keyed by resource name
    plugins: Arc<Mutex<HashMap<String, Arc<PluginConnection>>>>,
    /// Directory where the device plugin sockets live
    pub(crate) plugin_dir: PathBuf,
    /// Contains all the devices advertised by all device plugins. Key is resource name.
    /// Shared with the NodePatcher.
    pub(crate) devices: Arc<Mutex<DeviceMap>>,
    /// Structure containing map with Pod to currently allocated devices mapping
    pod_devices: PodDevices,
    /// Devices that have been allocated to Pods, keyed by resource name.
    allocated_device_ids: Arc<Mutex<DeviceIdMap>>,
    /// Sender to notify the NodePatcher to update NodeStatus with latest resource values.
    update_node_status_sender: broadcast::Sender<()>,
    /// Struture that patches the Node with the latest resource values when signaled.
    pub(crate) node_status_patcher: NodeStatusPatcher,
}

impl DeviceManager {
    /// Returns a new device manager configured with the given device plugin directory path
    pub fn new<P: AsRef<Path>>(plugin_dir: P, client: kube::Client, node_name: &str) -> Self {
        let devices = Arc::new(Mutex::new(HashMap::new()));
        let (update_node_status_sender, _) = broadcast::channel(UPDATE_NODE_STATUS_CHANNEL_SIZE);
        let node_status_patcher = NodeStatusPatcher::new(
            node_name,
            devices.clone(),
            update_node_status_sender.clone(),
            client.clone(),
        );
        let pod_devices = PodDevices::new(node_name, client);
        DeviceManager {
            plugin_dir: PathBuf::from(plugin_dir.as_ref()),
            plugins: Arc::new(Mutex::new(HashMap::new())),
            devices,
            pod_devices,
            allocated_device_ids: Arc::new(Mutex::new(HashMap::new())),
            update_node_status_sender,
            node_status_patcher,
        }
    }

    /// Returns a new device manager configured with the default `/var/lib/kubelet/device_plugins/` device plugin directory path
    pub fn new_with_default_path(client: kube::Client, node_name: &str) -> Self {
        DeviceManager::new(DEFAULT_PLUGIN_PATH, client, node_name)
    }

    /// Adds the plugin to our HashMap
    fn add_plugin(&self, plugin_connection: Arc<PluginConnection>, resource_name: &str) {
        let mut lock = self.plugins.lock().unwrap();
        lock.insert(resource_name.to_string(), plugin_connection);
    }

    /// Validates the given plugin info gathered from a discovered plugin, returning an error with
    /// additional information if it is not valid. This will validate 3 specific things (should
    /// answer YES to all of these):
    /// 1. Does this manager support the device plugin version? Currently only accepting `API_VERSION`.
    ///    TODO: determine whether can support all versions prior to current `API_VERSION`.
    /// 2. Does the plugin have a valid extended resource name?
    /// 3. Is the plugin name available? 2a. If the name is already registered, is the plugin_connection the
    ///    exact same? If it is, we allow it to reregister
    pub(crate) async fn validate(
        &self,
        register_request: &RegisterRequest,
    ) -> Result<(), tonic::Status> {
        trace!(
            "Starting validation for plugin {:?} discovered at path {}",
            register_request.resource_name,
            register_request.endpoint
        );

        // Validate that version matches the Device Plugin API version
        if register_request.version != API_VERSION {
            return Err(tonic::Status::new(
                tonic::Code::Unimplemented,
                format!(
                    "kubelet doesn't support version Device Plugin {}",
                    API_VERSION
                ),
            ));
        };

        // Validate that plugin has proper extended resource name
        if !super::resources::is_extended_resource_name(&register_request.resource_name) {
            return Err(tonic::Status::new(
                tonic::Code::Unimplemented,
                format!(
                    "resource name {} is not properly formatted. See https://github.com/kubernetes/community/blob/master/contributors/design-proposals/scheduling/resources.md#resource-types", register_request.resource_name)
            ));
        }

        self.validate_is_unique(register_request).await?;

        Ok(())
    }

    /// Validates if the plugin is unique (meaning it doesn't exist in the `plugins` map).
    /// If there is an active plugin registered with this name, returns error.
    /// TODO: Might be best to always accept: https://sourcegraph.com/github.com/kubernetes/kubernetes@9d6e5049bb719abf41b69c91437d25e273829746/-/blob/pkg/kubelet/cm/devicemanager/manager.go?subtree=true#L439
    async fn validate_is_unique(
        &self,
        register_request: &RegisterRequest,
    ) -> Result<(), tonic::Status> {
        let plugins = self.plugins.lock().unwrap();

        if let Some(_previous_plugin_entry) = plugins.get(&register_request.resource_name) {
            // TODO: check if plugin is active
            return Err(tonic::Status::new(
                tonic::Code::AlreadyExists,
                format!(
                    "Device Plugin with resource name {} already registered",
                    register_request.resource_name
                ),
            ));
        }

        Ok(())
    }

    /// This creates a connection to a device plugin by calling it's ListAndWatch function.
    /// Upon a successful connection, an `PluginConnection` is added to the `plugins` map.
    pub(crate) async fn create_plugin_connection(
        &self,
        register_request: RegisterRequest,
    ) -> anyhow::Result<()> {
        trace!(
            "Connecting to plugin at {:?} for ListAndWatch",
            register_request.endpoint
        );
        let chan = grpc_sock::client::socket_channel(register_request.endpoint.clone()).await?;
        let client = DevicePluginClient::new(chan);

        // Clone structures for PluginConnection thread
        let all_devices = self.devices.clone();
        let update_node_status_sender = self.update_node_status_sender.clone();
        let resource_name = register_request.resource_name.clone();
        let plugin_connection = Arc::new(PluginConnection::new(client, register_request));
        let plugins = self.plugins.clone();
        let devices = self.devices.clone();
        self.add_plugin(plugin_connection.clone(), &resource_name);

        // TODO: decide whether to join/store all spawned ListAndWatch threads
        tokio::spawn(async move {
            plugin_connection
                .start(all_devices, update_node_status_sender.clone())
                .await;

            remove_plugin(plugins, &resource_name, plugin_connection);

            // ?? Should devices be marked unhealthy first?
            remove_resource(devices, &resource_name);

            // Update nodes status with devices removed
            update_node_status_sender.send(()).unwrap();
        });

        Ok(())
    }

    /// This is the call that you can use to allocate a set of devices
    /// from the registered device plugins.
    /// Takes in a map of devices requested by containers, keyed by container name.
    pub async fn do_allocate(
        &self,
        pod: &Pod,
        container_devices: PodResourceRequests,
    ) -> anyhow::Result<()> {
        let mut all_allocate_requests: HashMap<String, Vec<ContainerAllocateInfo>> = HashMap::new();
        let mut updated_allocated_devices = false;
        for (container_name, requested_resources) in container_devices {
            for (resource_name, quantity) in requested_resources {
                if !self.is_device_plugin_resource(&resource_name).await {
                    continue;
                }

                // Device plugin resources should be request in numerical amounts.
                // Return error if requested quantity cannot be parsed.
                let num_requested: usize = serde_json::to_string(&quantity)?.parse()?;

                // Check that the resource has enough healthy devices
                if !self
                    .is_healthy_resource(&resource_name, num_requested)
                    .await
                {
                    continue;
                }

                if !updated_allocated_devices {
                    // Only need to update allocated devices once
                    self.update_allocated_devices().await?;
                    updated_allocated_devices = true;
                }

                let devices_to_allocate = self
                    .devices_to_allocate(
                        &resource_name,
                        &pod.pod_uid(),
                        &container_name,
                        num_requested,
                    )
                    .await?;
                let container_allocate_request = ContainerAllocateRequest {
                    devices_i_ds: devices_to_allocate,
                };
                let mut container_requests = vec![ContainerAllocateInfo {
                    container_name: container_name.clone(),
                    container_allocate_request,
                }];
                all_allocate_requests
                    .entry(resource_name)
                    .and_modify(|v| v.append(&mut container_requests))
                    .or_insert(container_requests);
            }
        }

        // Reset allocated_device_ids if allocation fails
        if let Err(e) = self
            .do_allocate_for_pod(&pod.pod_uid(), all_allocate_requests)
            .await
        {
            let mut allocated_device_ids = self.allocated_device_ids.lock().unwrap();
            *allocated_device_ids = self.pod_devices.get_allocated_devices();
            Err(e)
        } else {
            Ok(())
        }
    }

    /// Allocates each resource requested by a Pods containers by calling allocate on the respective device plugins.
    /// Stores the allocate responces in the PodDevices allocated map.
    /// Returns an error if an allocate call to any device plugin returns an error.
    ///
    /// Note, say DP1 and DP2 are registered for R1 and R2 respectively. If the allocate call to DP1 for R1 succeeds
    /// but the allocate call to DP2 for R2 fails, DP1 will have completed any allocation steps it performs despite the fact that the
    /// Pod will not be scheduled due to R2 not being an available resource.
    async fn do_allocate_for_pod(
        &self,
        pod_uid: &str,
        all_allocate_requests: HashMap<String, Vec<ContainerAllocateInfo>>,
    ) -> anyhow::Result<()> {
        let mut container_devices: ContainerDevices = HashMap::new();
        for (resource_name, container_allocate_info) in all_allocate_requests {
            let plugin_connection = self
                .plugins
                .lock()
                .unwrap()
                .get(&resource_name)
                .unwrap()
                .clone();
            let container_requests = container_allocate_info
                .iter()
                .map(|container_allocate_info| {
                    container_allocate_info.container_allocate_request.clone()
                })
                .collect::<Vec<ContainerAllocateRequest>>();
            let container_names = container_allocate_info
                .iter()
                .map(|container_allocate_info| &container_allocate_info.container_name)
                .collect::<Vec<&String>>();
            let allocate_request = AllocateRequest {
                container_requests: container_requests.clone(),
            };
            let allocate_response = plugin_connection.allocate(allocate_request).await?;
            allocate_response
                .into_inner()
                .container_responses
                .into_iter()
                .enumerate()
                .for_each(|(i, container_resp)| {
                    let device_allocate_info = DeviceAllocateInfo {
                        device_ids: container_requests[i]
                            .devices_i_ds
                            .clone()
                            .into_iter()
                            .collect::<HashSet<String>>(),
                        allocate_response: container_resp,
                    };
                    container_devices
                        .entry(container_names[i].clone())
                        .and_modify(|m| {
                            m.insert(resource_name.clone(), device_allocate_info.clone());
                        })
                        .or_insert_with(|| {
                            let mut m = HashMap::new();
                            m.insert(resource_name.clone(), device_allocate_info);
                            m
                        });
                });
        }
        self.pod_devices
            .add_allocated_devices(pod_uid, container_devices);
        Ok(())
    }

    /// Checks that a resource with the given name exists in the device plugin map
    async fn is_device_plugin_resource(&self, resource_name: &str) -> bool {
        self.devices.lock().unwrap().get(resource_name).is_some()
    }

    /// Asserts that the resource is in the device map and has at least `quantity` healthy devices.
    /// Later a check is made to make sure enough have not been allocated yet.
    async fn is_healthy_resource(&self, resource_name: &str, quantity: usize) -> bool {
        if let Some(resource_devices) = self.devices.lock().unwrap().get(resource_name) {
            if resource_devices
                .iter()
                .filter_map(
                    |(_id, dev)| {
                        if dev.health == HEALTHY {
                            Some(1)
                        } else {
                            None
                        }
                    },
                )
                .sum::<usize>()
                > quantity
            {
                return true;
            }
        }
        false
    }

    /// Frees any Devices that are bound to terminated pods.
    async fn update_allocated_devices(&self) -> anyhow::Result<()> {
        let active_pods = self.pod_devices.get_active_pods().await?;
        let mut pods_to_be_removed = self.pod_devices.get_pods();
        // remove all active pods from the list of pods to be removed
        active_pods.iter().for_each(|p_uid| {
            pods_to_be_removed.remove(p_uid);
        });

        if pods_to_be_removed.is_empty() {
            return Ok(());
        }

        self.pod_devices.remove_pods(pods_to_be_removed.clone())?;

        // TODO: should `allocated_device_ids` be replaced with `self.pod_devices.get_allocated_devices` instead?
        let mut allocated_device_ids = self.allocated_device_ids.lock().unwrap();
        pods_to_be_removed.iter().for_each(|p_uid| {
            allocated_device_ids.remove(p_uid);
        });
        Ok(())
    }

    /// Returns all of the allocate responses for a Pod. Used to set mounts, env vars, annotations, and device specs for Pod.
    pub fn get_pod_allocate_responses(
        &self,
        pod_uid: &str,
    ) -> Option<Vec<ContainerAllocateResponse>> {
        self.pod_devices.get_pod_allocate_responses(pod_uid)
    }

    /// Looks to see if devices have been previously allocated to a container (due to a container restart)
    /// or for devices that are healthy and not yet allocated.
    /// Returns list of device Ids we need to allocate with Allocate rpc call.
    /// Returns empty list in case we don't need to issue the Allocate rpc call.
    async fn devices_to_allocate(
        &self,
        resource_name: &str,
        pod_uid: &str,
        container_name: &str,
        quantity: usize,
    ) -> anyhow::Result<Vec<String>> {
        // Get list of devices already allocated to this container. This can occur if a container has restarted.
        if let Some(device_ids) =
            self.pod_devices
                .get_container_devices(resource_name, pod_uid, container_name)
        {
            if quantity - device_ids.len() != 0 {
                return Err(anyhow::format_err!("Pod {} with container named {} changed requested quantity for resource {} from {} to {}", pod_uid, container_name, resource_name, device_ids.len(), quantity));
            } else {
                // No change, so no new work
                return Ok(Vec::new());
            }
        }
        // Grab lock on devices and allocated devices
        let resource_devices_map = self.devices.lock().unwrap();
        let resource_devices = resource_devices_map.get(resource_name).ok_or_else(|| {
            anyhow::format_err!(
                "Device plugin does not exist for resource {}",
                resource_name
            )
        })?;
        let mut allocated_devices_map = self.allocated_device_ids.lock().unwrap();
        let mut allocated_devices = allocated_devices_map
            .get(resource_name)
            .unwrap_or(&HashSet::new())
            .clone();
        // Get available devices
        let available_devices = get_available_devices(resource_devices, &allocated_devices);

        // Check that enough devices are available
        if available_devices.len() < quantity {
            return Err(anyhow::format_err!(
                "Pod {} requested {} devices for resource {}, but only {} available for resource",
                pod_uid,
                quantity,
                resource_name,
                available_devices.len()
            ));
        }

        // let plugin_connection = self.plugins.lock().unwrap().get(resource_name).unwrap().clone();
        // let get_preferred_allocation_available = match &plugin_connection.register_request.options {
        //     None => false,
        //     Some(options) => options.get_preferred_allocation_available,
        // };

        // if get_preferred_allocation_available {
        //
        // }

        // TODO: support preferred allocation
        // For now, reserve first N devices where N = quantity by adding them to allocated map
        let devices_to_allocate: Vec<String> = available_devices[..quantity].to_vec();
        devices_to_allocate.iter().for_each(|dev| {
            allocated_devices.insert(dev.clone());
        });
        allocated_devices_map.insert(resource_name.to_string(), allocated_devices);

        Ok(devices_to_allocate)
    }
}

/// Returns the device IDs of all healthy devices that have yet to be allocated.
fn get_available_devices(
    devices: &PluginDevicesMap,
    allocated_devices: &HashSet<String>,
) -> Vec<String> {
    let healthy_devices = devices
        .iter()
        .filter_map(|(dev_id, dev)| {
            if dev.health == HEALTHY {
                Some(dev_id.clone())
            } else {
                None
            }
        })
        .collect::<HashSet<String>>();
    healthy_devices
        .difference(allocated_devices)
        .into_iter()
        .cloned()
        .collect::<Vec<String>>()
}

/// Removes the PluginConnection from our HashMap so long as it is the same as the one currently
/// stored under the given resource name.
fn remove_plugin(
    plugins: Arc<Mutex<HashMap<String, Arc<PluginConnection>>>>,
    resource_name: &str,
    plugin_connection: Arc<PluginConnection>,
) {
    let mut lock = plugins.lock().unwrap();
    if let Some(old_plugin_connection) = lock.get(resource_name) {
        // TODO: partialEq only checks that reg requests are identical. May also want
        // to check match of other fields.
        if *old_plugin_connection == plugin_connection {
            lock.remove(resource_name);
        }
    }
}

/// Removed all devices of a resource from our shared device map.
fn remove_resource(devices: Arc<Mutex<DeviceMap>>, resource_name: &str) {
    match devices.lock().unwrap().remove(resource_name) {
        Some(_) => trace!(
            "All devices of resource {} have been removed",
            resource_name
        ),
        None => trace!(
            "All devices of resource {} were already removed",
            resource_name
        ),
    }
}
