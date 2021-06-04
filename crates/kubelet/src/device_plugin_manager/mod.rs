//! The Kubelet device plugin manager. Consists of a `DeviceRegistry` that hosts a registration service for device plugins, a `DeviceManager` that maintains a device plugin client for each registered device plugin, a `NodePatcher` that patches the Node status with the extended resources advertised by device plugins, and a `PodDevices` that maintains a list of Pods that are actively using allocated resources.
mod node_patcher;
mod pod_devices;
pub mod resources;
use crate::device_plugin_api::v1beta1::{
    device_plugin_client::DevicePluginClient,
    registration_server::{Registration, RegistrationServer},
    AllocateRequest, ContainerAllocateRequest, Device, Empty, RegisterRequest, API_VERSION,
};
use crate::grpc_sock;
use crate::pod::Pod;

use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use node_patcher::NodeStatusPatcher;
use pod_devices::{ContainerDevices, DeviceAllocateInfo, PodDevices};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;
use tokio::task;
#[cfg(target_family = "windows")]
use tokio_compat_02::FutureExt;
use tonic::{transport::Server, Request};
use tracing::{error, trace};

#[cfg(target_family = "unix")]
const DEFAULT_PLUGIN_PATH: &str = "/var/lib/kubelet/device_plugins/";
#[cfg(target_family = "windows")]
const DEFAULT_PLUGIN_PATH: &str = "c:\\ProgramData\\kubelet\\device_plugins";

const PLUGIN_MANGER_SOCKET_NAME: &str = "kubelet.sock";
const UPDATE_NODE_STATUS_CHANNEL_SIZE: usize = 15;

/// `DeviceIdMap` contains the device Ids of all the devices advertised by device plugins.
/// Key is resource name.
type DeviceIdMap = HashMap<String, EndpointDeviceIds>;

/// `EndpointDeviceIds` contains the IDs of all the devices advertised by a single device plugin
type EndpointDeviceIds = HashSet<String>;

/// `DeviceMap` contains all the devices advertised by all device plugins. Key is resource name.
type DeviceMap = HashMap<String, EndpointDevicesMap>;

/// `EndpointDevicesMap` contains all of the devices advertised by a single device plugin. Key is device ID.
type EndpointDevicesMap = HashMap<String, Device>;

/// Healthy means the device is allocatable (whether already allocated or not)
const HEALTHY: &str = "Healthy";

/// Unhealthy means the device is not allocatable
/// TODO: use when device plugins go offline
const UNHEALTHY: &str = "Unhealthy";

/// Endpoint that maps to a single registered device plugin.
/// It is responsible for managing gRPC communications with the device plugin and caching
/// device states reported by the device plugin
#[derive(Clone)]
struct Endpoint {
    /// Client that is connected to the device plugin
    client: DevicePluginClient<tonic::transport::Channel>,
    /// `RegisterRequest` received when the device plugin registered with the DeviceRegistry
    register_request: RegisterRequest,
}

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
    plugins: Arc<Mutex<HashMap<String, Endpoint>>>,
    /// Directory where the device plugin sockets live
    plugin_dir: PathBuf,
    /// Contains all the devices advertised by all device plugins. Key is resource name.
    /// Shared with the NodePatcher.
    devices: Arc<Mutex<DeviceMap>>,
    /// Structure containing map with Pod to currently allocated devices mapping
    pub pod_devices: PodDevices,
    /// Devices that have been allocated to Pods, keyed by resource name.
    allocated_device_ids: Arc<Mutex<DeviceIdMap>>,
    /// Sender to notify the NodePatcher to update NodeStatus with latest resource values.
    update_node_status_sender: broadcast::Sender<()>,
    /// Struture that patches the Node with the latest resource values when signaled.
    node_status_patcher: NodeStatusPatcher,
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
        let pod_devices = PodDevices::new(client);
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
    fn add_plugin(&self, endpoint: Endpoint) {
        let mut lock = self.plugins.lock().unwrap();
        lock.insert(endpoint.register_request.resource_name.clone(), endpoint);
    }

    /// Removes the plugin from our HashMap
    fn remove_plugin(&self, resource_name: &str) {
        let mut lock = self.plugins.lock().unwrap();
        lock.remove(resource_name);
    }

    /// Validates the given plugin info gathered from a discovered plugin, returning an error with
    /// additional information if it is not valid. This will validate 3 specific things (should
    /// answer YES to all of these):
    /// 1. Does this manager support the device plugin version? Currently only accepting `API_VERSION`.
    ///    TODO: determine whether can support all versions prior to current `API_VERSION`.
    /// 2. Is the plugin name available? 2a. If the name is already registered, is the endpoint the
    ///    exact same? If it is, we allow it to reregister
    async fn validate(&self, register_request: &RegisterRequest) -> Result<(), tonic::Status> {
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

        // TODO: validate that plugin has proper extended resource name
        // https://github.com/kubernetes/kubernetes/blob/ea0764452222146c47ec826977f49d7001b0ea8c/pkg/kubelet/cm/devicemanager/manager.go#L309
        // TODO: validate that endpoint is in proper directory

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

    /// This creates a connection to a Device Plugin by calling its ListAndWatch function.
    /// Upon a successful connection, an `Endpoint` is added to the `plugins` map.
    /// The device plugin updates the kubelet periodically about the capacity and health of its resource.
    /// Upon updates, this propagates any changes into the `plugins` map and triggers the `NodePatcher` to
    /// patch the node with the latest values.
    async fn create_endpoint(&self, register_request: &RegisterRequest) -> anyhow::Result<()> {
        trace!(
            "Connecting to plugin at {:?} for ListAndWatch",
            register_request.endpoint
        );
        let chan = grpc_sock::client::socket_channel(register_request.endpoint.clone()).await?;
        let client = DevicePluginClient::new(chan);

        // Clone structures for ListAndWatch thread
        let mut list_and_watch_client = client.clone();
        let list_and_watch_resource_name = register_request.resource_name.clone();
        let all_devices = self.devices.clone();
        let update_node_status_sender = self.update_node_status_sender.clone();

        // TODO: make options an enum?
        let success: i8 = 0;
        let error: i8 = 1;
        let (successful_connection_sender, successful_connection_receiver): (
            tokio::sync::oneshot::Sender<i8>,
            tokio::sync::oneshot::Receiver<i8>,
        ) = tokio::sync::oneshot::channel();

        // TODO: decide whether to join all spawned ListAndWatch threads
        tokio::spawn(async move {
            match list_and_watch_client
                .list_and_watch(Request::new(Empty {}))
                .await
            {
                Err(e) => {
                    error!("could not call ListAndWatch on device plugin with resource name {:?} with error {}", list_and_watch_resource_name, e);
                    successful_connection_sender.send(error).unwrap();
                }
                Ok(stream_wrapped) => {
                    successful_connection_sender.send(success).unwrap();
                    let mut stream = stream_wrapped.into_inner();
                    let mut previous_endpoint_devices: HashMap<String, Device> = HashMap::new();
                    while let Some(response) = stream.message().await.unwrap() {
                        let current_devices = response
                            .devices
                            .iter()
                            .map(|device| (device.id.clone(), device.clone()))
                            .collect::<HashMap<String, Device>>();
                        let mut update_node_status = false;
                        // Iterate through the list of devices, updating the Node status if
                        // (1) Device modified: DP reporting a previous device with a different health status
                        // (2) Device added: DP reporting a new device
                        // (3) Device removed: DP is no longer advertising a device
                        current_devices.iter().for_each(|(_, device)| {
                            // (1) Device modified or already registered
                            if let Some(previous_device) = previous_endpoint_devices.get(&device.id)
                            {
                                if previous_device.health != device.health {
                                    all_devices
                                        .lock()
                                        .unwrap()
                                        .get_mut(&list_and_watch_resource_name)
                                        .unwrap()
                                        .insert(device.id.clone(), device.clone());
                                    update_node_status = true;
                                } else if previous_device.topology != device.topology {
                                    // TODO: how to handle this
                                    error!("device topology changed");
                                }
                            // (2) Device added
                            } else {
                                let mut all_devices_map = all_devices.lock().unwrap();
                                match all_devices_map.get_mut(&list_and_watch_resource_name) {
                                    Some(resource_devices_map) => {
                                        resource_devices_map
                                            .insert(device.id.clone(), device.clone());
                                    }
                                    None => {
                                        let mut resource_devices_map = HashMap::new();
                                        resource_devices_map
                                            .insert(device.id.clone(), device.clone());
                                        all_devices_map.insert(
                                            list_and_watch_resource_name.clone(),
                                            resource_devices_map,
                                        );
                                    }
                                }
                                update_node_status = true;
                            }
                        });

                        // (3) Check if Device removed
                        previous_endpoint_devices
                            .iter()
                            .for_each(|(_, previous_device)| {
                                if !response.devices.contains(previous_device) {
                                    // TODO: how to handle already allocated devices? Pretty sure K8s lets them keep running but what about the allocated_device map?
                                    all_devices
                                        .lock()
                                        .unwrap()
                                        .get_mut(&list_and_watch_resource_name)
                                        .unwrap()
                                        .remove(&previous_device.id);
                                    update_node_status = true;
                                }
                            });

                        // Replace previous devices with current devices
                        previous_endpoint_devices = current_devices;

                        if update_node_status {
                            // TODO handle error -- maybe channel is full
                            update_node_status_sender.send(()).unwrap();
                        }
                    }
                    // TODO: remove endpoint from map
                }
            }
        });

        // Only add device plugin to map if successful ListAndWatch call
        if successful_connection_receiver.await.unwrap() == success {
            let endpoint = Endpoint {
                client,
                register_request: register_request.clone(),
            };
            self.add_plugin(endpoint);
        } else {
            return Err(anyhow::Error::msg(format!(
                "could not call ListAndWatch on device plugin at socket {:?}",
                register_request.endpoint
            )));
        }

        Ok(())
    }

    /// This is the call that you can use to allocate a set of devices
    /// from the registered device plugins.
    /// Takes in a map of devices requested by containers, keyed by container name.
    pub async fn do_allocate(
        &self,
        pod: &Pod,
        container_devices: HashMap<String, HashMap<String, Quantity>>,
    ) -> anyhow::Result<()> {
        let mut all_allocate_requests: HashMap<String, Vec<ContainerAllocateInfo>> = HashMap::new();
        let mut updated_allocated_devices = false;
        for (container_name, requested_resources) in container_devices {
            for (resource_name, quantity) in requested_resources {
                let num_requested: usize =
                    serde_json::to_string(&quantity).unwrap().parse().unwrap();
                if !self
                    .is_device_plugin_resource(&resource_name, num_requested)
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
                if let Some(all_container_requests) = all_allocate_requests.get_mut(&resource_name)
                {
                    all_container_requests.append(&mut container_requests);
                } else {
                    all_allocate_requests.insert(resource_name.clone(), container_requests);
                }
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
    pub async fn do_allocate_for_pod(
        &self,
        pod_uid: &str,
        all_allocate_requests: HashMap<String, Vec<ContainerAllocateInfo>>,
    ) -> anyhow::Result<()> {
        let mut container_devices: ContainerDevices = HashMap::new();
        for (resource_name, container_allocate_info) in all_allocate_requests {
            let mut endpoint = self
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
            let allocate_response = endpoint
                .client
                .allocate(Request::new(allocate_request))
                .await?;
            let mut container_index = 0;
            allocate_response
                .into_inner()
                .container_responses
                .into_iter()
                .for_each(|container_resp| {
                    let device_allocate_info = DeviceAllocateInfo {
                        device_ids: container_requests[container_index]
                            .devices_i_ds
                            .clone()
                            .into_iter()
                            .collect::<HashSet<String>>(),
                        allocate_response: container_resp,
                    };
                    let container_name = container_names[container_index];
                    if let Some(resource_allocate_info) = container_devices.get_mut(container_name)
                    {
                        resource_allocate_info.insert(resource_name.clone(), device_allocate_info);
                    } else {
                        let mut resource_allocate_info: HashMap<String, DeviceAllocateInfo> =
                            HashMap::new();
                        resource_allocate_info.insert(resource_name.clone(), device_allocate_info);
                        container_devices.insert(container_name.clone(), resource_allocate_info);
                    }
                    container_index += 1;
                });
        }
        self.pod_devices
            .add_allocated_devices(pod_uid, container_devices);
        Ok(())
    }

    /// Asserts that the resource is in the device map and has at least one healthy device.
    /// Asserts that enough healthy devices are available.
    /// Later a check is made to make sure enough have not been allocated yet.
    async fn is_device_plugin_resource(&self, resource_name: &str, quantity: usize) -> bool {
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

        // let endpoint = self.plugins.lock().unwrap().get(resource_name).unwrap().clone();
        // let get_preferred_allocation_available = match &endpoint.register_request.options {
        //     None => false,
        //     Some(options) => options.get_preferred_allocation_available,
        // };

        // if get_preferred_allocation_available {
        //
        // }

        // TODO: support preferred allocation
        // For now, reserve first N devices where N = quantity by adding them to allocated map
        let devices_to_allocate: Vec<String> = available_devices[..quantity].to_vec();
        // ??: is there a cleaner way to map `allocated_devices.insert(dev.clone())` from bool to
        // () so can remove {} block:
        devices_to_allocate.iter().for_each(|dev| {
            allocated_devices.insert(dev.clone());
        });
        allocated_devices_map.insert(resource_name.to_string(), allocated_devices);

        Ok(devices_to_allocate)
    }
}

/// Returns the device IDs of all healthy devices that have yet to be allocated.
fn get_available_devices(
    devices: &EndpointDevicesMap,
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

/// Hosts the device plugin `Registration` service (defined in the device plugin API) for a `DeviceManager`.
/// Upon device plugin registration, reaches out to its `DeviceManager` to validate the device plugin
/// and establish a connection with it.
#[derive(Clone)]
pub struct DeviceRegistry {
    device_manager: Arc<DeviceManager>,
}

impl DeviceRegistry {
    /// Returns a new `DeviceRegistry` with a reference to a `DeviceManager` to handle each registered device plugin.
    pub fn new(device_manager: Arc<DeviceManager>) -> Self {
        DeviceRegistry { device_manager }
    }
}
#[async_trait::async_trait]

impl Registration for DeviceRegistry {
    async fn register(
        &self,
        request: tonic::Request<RegisterRequest>,
    ) -> Result<tonic::Response<Empty>, tonic::Status> {
        let register_request = request.get_ref();
        // Validate
        self.device_manager
            .validate(register_request)
            .await
            .map_err(|e| tonic::Status::new(tonic::Code::InvalidArgument, format!("{}", e)))?;
        // Create a list and watch connection with the device plugin
        // TODO: should the manager keep track of threads?
        self.device_manager
            .create_endpoint(register_request)
            .await
            .map_err(|e| tonic::Status::new(tonic::Code::NotFound, format!("{}", e)))?;
        Ok(tonic::Response::new(Empty {}))
    }
}

/// Starts the `DeviceManager` by running its `NodePatcher` and serving the `DeviceRegistry` which hosts the device plugin manager's `Registration` service on the socket
/// specified in the `DeviceManager`.
/// Returns an error if either the `NodePatcher` or `DeviceRegistry` error.
pub async fn serve_device_registry(device_manager: Arc<DeviceManager>) -> anyhow::Result<()> {
    // TODO determine if need to create socket (and delete any previous ones)
    let manager_socket = device_manager.plugin_dir.join(PLUGIN_MANGER_SOCKET_NAME);
    let socket =
        grpc_sock::server::Socket::new(&manager_socket).expect("couldn't make manager socket");
    let node_status_patcher = device_manager.node_status_patcher.clone();
    let node_patcher_task = task::spawn(async move {
        node_status_patcher.listen_and_patch().await.unwrap();
    });
    let device_registry = DeviceRegistry::new(device_manager);
    // TODO: There may be a slight race case here. If the DeviceManager tries to send device info to the NodeStatusPatcher before the NodeStatusPatcher has created a receiver
    // it will error because there are no active receivers.
    let device_manager_task = task::spawn(async {
        let serv = Server::builder()
            .add_service(RegistrationServer::new(device_registry))
            .serve_with_incoming(socket);
        #[cfg(target_family = "windows")]
        let serv = serv.compat();
        serv.await.expect("Unable to serve device plugin manager");
    });
    tokio::try_join!(node_patcher_task, device_manager_task).map_err(anyhow::Error::from)?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::device_plugin_api::v1beta1::{
        device_plugin_server::{DevicePlugin, DevicePluginServer},
        registration_client, AllocateRequest, AllocateResponse, DevicePluginOptions, Empty,
        ListAndWatchResponse, PreStartContainerRequest, PreStartContainerResponse,
        PreferredAllocationRequest, PreferredAllocationResponse,
    };
    use futures::{pin_mut, Stream};
    use http::{Request as HttpRequest, Response as HttpResponse};
    use hyper::Body;
    use kube::Client;
    use std::pin::Pin;
    use tokio::sync::{mpsc, watch};
    use tonic::{Request, Response, Status};
    use tower_test::mock;

    /// Mock Device Plugin for testing the DeviceManager
    /// Sends a new list of devices to the DeviceManager whenever it's `devices_receiver`
    /// is notified of them on a channel.
    pub struct MockDevicePlugin {
        // Using watch so the receiver can be cloned and be moved into a spawned thread in ListAndWatch
        devices_receiver: watch::Receiver<Vec<Device>>,
    }

    #[async_trait::async_trait]
    impl DevicePlugin for MockDevicePlugin {
        async fn get_device_plugin_options(
            &self,
            _request: Request<Empty>,
        ) -> Result<Response<DevicePluginOptions>, Status> {
            unimplemented!();
        }

        type ListAndWatchStream = Pin<
            Box<dyn Stream<Item = Result<ListAndWatchResponse, Status>> + Send + Sync + 'static>,
        >;
        async fn list_and_watch(
            &self,
            _request: Request<Empty>,
        ) -> Result<Response<Self::ListAndWatchStream>, Status> {
            trace!("list_and_watch entered");
            // Create a channel that list_and_watch can periodically send updates to kubelet on
            let (kubelet_update_sender, kubelet_update_receiver) = mpsc::channel(3);
            let mut devices_receiver = self.devices_receiver.clone();
            tokio::spawn(async move {
                while devices_receiver.changed().await.is_ok() {
                    let devices = devices_receiver.borrow().clone();
                    trace!(
                        "list_and_watch received new devices [{:?}] to send",
                        devices
                    );
                    kubelet_update_sender
                        .send(Ok(ListAndWatchResponse { devices }))
                        .await
                        .unwrap();
                }
            });
            Ok(Response::new(Box::pin(
                tokio_stream::wrappers::ReceiverStream::new(kubelet_update_receiver),
            )))
        }

        async fn get_preferred_allocation(
            &self,
            _request: Request<PreferredAllocationRequest>,
        ) -> Result<Response<PreferredAllocationResponse>, Status> {
            unimplemented!();
        }

        async fn allocate(
            &self,
            _request: Request<AllocateRequest>,
        ) -> Result<Response<AllocateResponse>, Status> {
            unimplemented!();
        }

        async fn pre_start_container(
            &self,
            _request: Request<PreStartContainerRequest>,
        ) -> Result<Response<PreStartContainerResponse>, Status> {
            Ok(Response::new(PreStartContainerResponse {}))
        }
    }

    /// Serves the mock DP
    async fn run_mock_device_plugin(
        socket_path: impl AsRef<Path>,
        devices_receiver: watch::Receiver<Vec<Device>>,
    ) -> anyhow::Result<()> {
        let device_plugin = MockDevicePlugin { devices_receiver };
        let socket = grpc_sock::server::Socket::new(&socket_path).expect("couldnt make dp socket");
        let serv = Server::builder()
            .add_service(DevicePluginServer::new(device_plugin))
            .serve_with_incoming(socket);
        #[cfg(target_family = "windows")]
        let serv = serv.compat();
        serv.await.expect("Unable to serve mock device plugin");
        Ok(())
    }

    /// Registers the mock DP with the DeviceManager's registration service
    async fn register_mock_device_plugin(
        kubelet_socket: String,
        dp_socket: &str,
        dp_resource_name: &str,
    ) -> anyhow::Result<()> {
        let op = DevicePluginOptions {
            get_preferred_allocation_available: false,
            pre_start_required: false,
        };
        let channel = grpc_sock::client::socket_channel(kubelet_socket).await?;
        let mut registration_client = registration_client::RegistrationClient::new(channel);
        let register_request = tonic::Request::new(RegisterRequest {
            version: API_VERSION.into(),
            endpoint: dp_socket.to_string(),
            resource_name: dp_resource_name.to_string(),
            options: Some(op),
        });
        registration_client
            .register(register_request)
            .await
            .unwrap();
        Ok(())
    }

    /// Creates a mock kubernetes API service that the NodePatcher calls to when device plugin resources
    /// need to be updated in the Node status.
    /// It verifies the request and always returns a fake Node.
    /// Returns a client that will reference this mock service and the task the service is running on.
    /// TODO: Decide whether to test node status
    pub async fn create_mock_kube_service(
        node_name: &str,
    ) -> (Client, tokio::task::JoinHandle<()>) {
        // Mock client as inspired by this thread on kube-rs crate: https://github.com/clux/kube-rs/issues/429
        let (mock_service, handle) = mock::pair::<HttpRequest<Body>, HttpResponse<Body>>();
        let node_name = node_name.to_string();
        let spawned = tokio::spawn(async move {
            pin_mut!(handle);
            let (request, send) = handle.next_request().await.expect("service not called");
            assert_eq!(request.method(), http::Method::PATCH);
            assert_eq!(
                request.uri().to_string(),
                format!("/api/v1/nodes/{}/status?", node_name)
            );
            let node: k8s_openapi::api::core::v1::Node =
                serde_json::from_value(serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "Node",
                    "metadata": {
                        "name": "test",
                    }
                }))
                .unwrap();
            send.send_response(
                HttpResponse::builder()
                    .body(Body::from(serde_json::to_vec(&node).unwrap()))
                    .unwrap(),
            );
        });
        let client = Client::new(mock_service);
        (client, spawned)
    }

    /// Tests e2e flow of kicked off by a mock DP registering with the DeviceManager
    /// DeviceManager should call ListAndWatch on the DP, update it's devices registry with the DP's
    /// devices, and instruct it's NodePatcher to patch the node status with the new DP resources.
    #[tokio::test]
    async fn do_device_manager_test() {
        // There doesn't seem to be a way to use the same temp dir for manager and mock dp due to being able to
        // pass the temp dir reference to multiple threads
        // Instead, create a temp dir for the DP manager and the mock DP
        let device_plugin_temp_dir = tempfile::tempdir().expect("should be able to create tempdir");
        let manager_temp_dir = tempfile::tempdir().expect("should be able to create tempdir");

        // Capture the DP and DP manager socket paths
        let socket_name = "gpu-device-plugin.sock";
        let dp_socket = device_plugin_temp_dir
            .path()
            .join(socket_name)
            .to_str()
            .unwrap()
            .to_string();
        let manager_socket = manager_temp_dir
            .path()
            .join(PLUGIN_MANGER_SOCKET_NAME)
            .to_str()
            .unwrap()
            .to_string();

        // Make 3 mock devices
        let d1 = Device {
            id: "d1".to_string(),
            health: HEALTHY.to_string(),
            topology: None,
        };
        let d2 = Device {
            id: "d2".to_string(),
            health: HEALTHY.to_string(),
            topology: None,
        };
        let d3 = Device {
            id: "d3".to_string(),
            health: UNHEALTHY.to_string(),
            topology: None,
        };

        // Start the mock device plugin without any devices
        let devices: Vec<Device> = Vec::new();
        let (devices_sender, devices_receiver) = watch::channel(devices);

        // Run the mock device plugin
        let device_plugin_task = task::spawn(async move {
            run_mock_device_plugin(
                device_plugin_temp_dir.path().join(socket_name),
                devices_receiver,
            )
            .await
            .unwrap();
        });

        // Name of "this node" that should be patched with Device Plugin resources
        let test_node_name = "test_node";
        // Create and run a mock Kubernetes API service and get a Kubernetes client
        let (client, mock_service_task) = create_mock_kube_service(test_node_name).await;

        // Create and serve a DeviceManager
        let device_manager = Arc::new(DeviceManager::new(
            manager_temp_dir.path().clone(),
            client,
            test_node_name,
        ));
        let devices = device_manager.devices.clone();
        let manager_task = task::spawn(async move {
            serve_device_registry(device_manager).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Register the mock device plugin with the DeviceManager's Registration service
        let dp_resource_name = "mock-device-plugin";
        register_mock_device_plugin(manager_socket.to_string(), &dp_socket, dp_resource_name)
            .await
            .unwrap();

        // Make DP report 2 healthy and 1 unhealthy device
        devices_sender.send(vec![d1, d2, d3]).unwrap();

        let mut x: i8 = 0;
        let mut num_devices: i8 = 0;
        while x < 3 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            // Assert that there are 3 devices in the map now
            if let Some(resource_devices_map) = devices.lock().unwrap().get(dp_resource_name) {
                if resource_devices_map.len() == 3 {
                    num_devices = 3;
                    break;
                }
            }
            x += 1;
        }
        assert_eq!(num_devices, 3);

        // tokio::join!(device_plugin_task, mock_service_task, manager_task);
    }
}
