//! The `DeviceManager` maintains a device plugin client for each registered device plugin. It
//! ensures that the Node's `NodeStatus` contains the the resources advertised by device plugins and
//! performs allocate calls on device plugins when Pods are scheduled that request device plugin
//! resources.
use super::super::util;
use super::node_patcher::NodeStatusPatcher;
use super::plugin_connection::PluginConnection;
use super::pod_devices::{ContainerDevices, DeviceAllocateInfo, PodDevices};
use super::{DeviceIdMap, DeviceMap, PluginDevicesMap, PodResourceRequests, HEALTHY};
use crate::device_plugin_api::v1beta1::{
    device_plugin_client::DevicePluginClient, AllocateRequest, ContainerAllocateRequest,
    ContainerAllocateResponse, RegisterRequest, API_VERSION,
};
use crate::grpc_sock;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use tokio::sync::broadcast;
use tracing::{debug, trace};

#[cfg(target_family = "unix")]
const DEFAULT_PLUGIN_PATH: &str = "/var/lib/kubelet/device-plugins/";
#[cfg(target_family = "windows")]
const DEFAULT_PLUGIN_PATH: &str = "c:\\ProgramData\\kubelet\\device-plugins";

const UPDATE_NODE_STATUS_CHANNEL_SIZE: usize = 15;

/// ContainerAllocateInfo pairs an allocate request to with the requesting container
#[derive(Clone)]
pub struct ContainerAllocateInfo {
    /// The name of the container
    container_name: String,
    /// The `ContainerAllocateRequest` sent to the device plugin for this container
    container_allocate_request: ContainerAllocateRequest,
}

/// An implementation of the Kubernetes Device Plugin Manager
/// (https://github.com/kubernetes/kubernetes/tree/v1.21.1/pkg/kubelet/cm/devicemanager). It
/// implements the device plugin framework's `Registration` gRPC service. A device plugin (DP) can
/// register itself with the kubelet through this gRPC service. This allows the DP to advertise a
/// resource like system hardware kubelet. The `DeviceManager` contains a `NodePatcher` that patches
/// the Node with resources advertised by DPs. Then the K8s scheduler can schedule Pods that request
/// those resources to this Node. Once scheduled, the device manager confirms notifies the DP that
/// it wants to use on of its resources by making an `allocate` gRPC call. On success, it ensures
/// that all necessary mounts, environment variables, annotations, and device specs are added to the
/// Pod (which's Containers) are requesting the DP resource.
#[derive(Clone)]
pub struct DeviceManager {
    /// Map of registered device plugins, keyed by resource name
    plugins: Arc<RwLock<HashMap<String, Arc<PluginConnection>>>>,
    /// Directory where the device plugin sockets live
    pub(crate) plugin_dir: PathBuf,
    /// Contains all the devices advertised by all device plugins. Key is resource name. Shared with
    /// the NodePatcher.
    pub(crate) devices: Arc<RwLock<DeviceMap>>,
    /// Structure containing map with Pod to currently allocated devices mapping
    pod_devices: PodDevices,
    /// Devices that have been allocated to Pods, keyed by resource name.
    allocated_device_ids: Arc<RwLock<DeviceIdMap>>,
    /// Sender to notify the NodePatcher to update NodeStatus with latest resource values.
    update_node_status_sender: broadcast::Sender<()>,
    /// Structure that patches the Node with the latest resource values when signaled.
    pub(crate) node_status_patcher: NodeStatusPatcher,
}

impl DeviceManager {
    /// Returns a new device manager configured with the given device plugin directory path
    pub fn new<P: AsRef<Path>>(plugin_dir: P, client: kube::Client, node_name: &str) -> Self {
        let devices = Arc::new(RwLock::new(HashMap::new()));
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
            plugins: Arc::new(RwLock::new(HashMap::new())),
            devices,
            pod_devices,
            allocated_device_ids: Arc::new(RwLock::new(HashMap::new())),
            update_node_status_sender,
            node_status_patcher,
        }
    }

    /// Returns a new device manager configured with the default `/var/lib/kubelet/device_plugins/`
    /// device plugin directory path
    pub fn new_with_default_path(client: kube::Client, node_name: &str) -> Self {
        DeviceManager::new(DEFAULT_PLUGIN_PATH, client, node_name)
    }

    /// Adds the plugin to our HashMap
    async fn add_plugin(&self, plugin_connection: Arc<PluginConnection>, resource_name: &str) {
        let mut lock = self.plugins.write().await;
        lock.insert(resource_name.to_string(), plugin_connection);
    }

    /// Validates the given plugin info gathered from a discovered plugin, returning an error with
    /// additional information if it is not valid. This will validate 3 specific things (should
    /// answer YES to all of these):
    /// 1. Does this manager support the device plugin version? Currently only accepting
    ///    `API_VERSION`. TODO: determine whether can support all versions prior to current
    ///    `API_VERSION`.
    /// 2. Does the plugin have a valid extended resource name?
    /// 3. Is the plugin name available? If the name is already registered, return an error that the
    ///    plugin already exists.
    pub(crate) async fn validate(
        &self,
        register_request: &RegisterRequest,
    ) -> Result<(), tonic::Status> {
        trace!(
            resource = %register_request.resource_name, endpoint = %register_request.endpoint, "Starting validation for plugin discovered at path",
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
        if !util::is_extended_resource_name(&register_request.resource_name) {
            return Err(tonic::Status::new(
                tonic::Code::Unimplemented,
                format!(
                    "resource name {} is not properly formatted. See https://github.com/kubernetes/community/blob/master/contributors/design-proposals/scheduling/resources.md#resource-types", register_request.resource_name)
            ));
        }

        self.validate_is_unique(register_request).await?;

        Ok(())
    }

    /// Validates that the plugin is unique (meaning it doesn't exist in the `plugins` map). If
    /// there is an active plugin registered with this name, returns error.
    async fn validate_is_unique(
        &self,
        register_request: &RegisterRequest,
    ) -> Result<(), tonic::Status> {
        let plugins = self.plugins.read().await;

        if let Some(_previous_plugin_entry) = plugins.get(&register_request.resource_name) {
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

    /// This creates a connection to a device plugin by calling it's ListAndWatch function. Upon a
    /// successful connection, an `PluginConnection` is added to the `plugins` map.
    pub(crate) async fn create_plugin_connection(
        &self,
        register_request: RegisterRequest,
    ) -> anyhow::Result<()> {
        debug!(
            resource = %register_request.resource_name, endpoint = %register_request.endpoint, "Connecting to plugin's ListAndWatch service"
        );
        let chan = grpc_sock::client::socket_channel(
            self.plugin_dir.join(register_request.endpoint.clone()),
        )
        .await?;
        let client = DevicePluginClient::new(chan);

        // Clone structures for PluginConnection thread
        let all_devices = self.devices.clone();
        let update_node_status_sender = self.update_node_status_sender.clone();
        let resource_name = register_request.resource_name.clone();
        let plugin_connection = Arc::new(PluginConnection::new(client, register_request));
        let plugins = self.plugins.clone();
        let devices = self.devices.clone();
        self.add_plugin(plugin_connection.clone(), &resource_name)
            .await;

        tokio::spawn(async move {
            plugin_connection
                .start(all_devices, update_node_status_sender.clone())
                .await;

            remove_plugin(plugins, &resource_name, plugin_connection).await;

            // This clears the map of devices for a resource.
            remove_resource_devices(devices, &resource_name).await;

            // Update nodes status with devices removed
            update_node_status_sender.send(()).unwrap();
        });

        Ok(())
    }

    /// This is the call that you can use to allocate a set of devices from the registered device
    /// plugins. Takes in a map of devices requested by containers, keyed by container name.
    pub async fn do_allocate(
        &self,
        pod_uid: &str,
        container_devices: PodResourceRequests,
    ) -> anyhow::Result<()> {
        debug!("do_allocate called for pod {}", pod_uid);
        let mut all_allocate_requests: HashMap<String, Vec<ContainerAllocateInfo>> = HashMap::new();
        let mut updated_allocated_devices = false;
        for (container_name, requested_resources) in container_devices {
            for (resource_name, quantity) in requested_resources {
                if !self.is_device_plugin_resource(&resource_name).await {
                    continue;
                }

                // Device plugin resources should be request in numerical amounts. Return error if
                // requested quantity cannot be parsed.
                let num_requested: usize = get_num_from_quantity(quantity)?;

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
                    .devices_to_allocate(&resource_name, pod_uid, &container_name, num_requested)
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
            .do_allocate_for_pod(pod_uid, all_allocate_requests)
            .await
        {
            *self.allocated_device_ids.write().await = self.pod_devices.get_allocated_devices();
            Err(e)
        } else {
            Ok(())
        }
    }

    /// Allocates each resource requested by a Pods containers by calling allocate on the respective
    /// device plugins. Stores the allocate responses in the PodDevices allocated map. Returns an
    /// error if an allocate call to any device plugin returns an error.
    ///
    /// Note, say DP1 and DP2 are registered for R1 and R2 respectively. If the allocate call to DP1
    /// for R1 succeeds but the allocate call to DP2 for R2 fails, DP1 will have completed any
    /// allocation steps it performs despite the fact that the Pod will not be scheduled due to R2
    /// not being an available resource. This is expected behavior with the device plugin interface.
    async fn do_allocate_for_pod(
        &self,
        pod_uid: &str,
        all_allocate_requests: HashMap<String, Vec<ContainerAllocateInfo>>,
    ) -> anyhow::Result<()> {
        let mut container_devices: ContainerDevices = HashMap::new();
        for (resource_name, container_allocate_info) in all_allocate_requests {
            let plugin_connection = self
                .plugins
                .read()
                .await
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
            let container_responses = plugin_connection
                .allocate(allocate_request)
                .await?
                .into_inner()
                .container_responses;

            // By doing one allocate call per Pod, an assumption is being made that the container
            // requests array (sent to a DP) and container responses array returned are the same
            // length. This is not documented in the DP API. However Kubernetes has a TODO
            // (https://github.com/kubernetes/kubernetes/blob/d849d9d057369121fc43aa3359059471e1ca9d1c/pkg/kubelet/cm/devicemanager/manager.go#L916)
            // to use the same one call implementation.
            if container_requests.len() != container_responses.len() {
                return Err(anyhow::anyhow!("Container responses returned from allocate are not the same length as container requests"));
            }

            container_responses
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
        self.devices.read().await.get(resource_name).is_some()
    }

    /// Asserts that the resource is in the device map and has at least `quantity` healthy devices.
    /// Later a check is made to make sure enough have not been allocated yet.
    async fn is_healthy_resource(&self, resource_name: &str, quantity: usize) -> bool {
        if let Some(resource_devices) = self.devices.read().await.get(resource_name) {
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

        self.pod_devices.remove_pods(pods_to_be_removed)?;

        Ok(())
    }

    /// Returns a map all of the allocate responses for a Pod, keyed by Container name. Used to set
    /// mounts, env vars, annotations, and device specs for Pod.
    pub fn get_pod_allocate_responses(
        &self,
        pod_uid: &str,
    ) -> Option<HashMap<String, Vec<ContainerAllocateResponse>>> {
        self.pod_devices.get_pod_allocate_responses(pod_uid)
    }

    /// Looks to see if devices have been previously allocated to a container (due to a container
    /// restart) or for devices that are healthy and not yet allocated. Returns list of device Ids
    /// we need to allocate with Allocate rpc call. Returns empty list in case we don't need to
    /// issue the Allocate rpc call.
    async fn devices_to_allocate(
        &self,
        resource_name: &str,
        pod_uid: &str,
        container_name: &str,
        quantity: usize,
    ) -> anyhow::Result<Vec<String>> {
        // Get list of devices already allocated to this container. This can occur if a container
        // has restarted.
        if let Some(device_ids) =
            self.pod_devices
                .get_container_devices(resource_name, pod_uid, container_name)
        {
            if quantity - device_ids.len() != 0 {
                return Err(anyhow::format_err!("Pod {} with container named {} changed requested quantity for resource {} from {} to {}", pod_uid, container_name, resource_name, device_ids.len(), quantity));
            } else {
                // No change, so no new work
                return Ok(Vec::with_capacity(0));
            }
        }
        // Grab lock on devices and allocated devices
        let resource_devices_map = self.devices.write().await;
        let resource_devices = resource_devices_map.get(resource_name).ok_or_else(|| {
            anyhow::format_err!(
                "Device plugin does not exist for resource {}",
                resource_name
            )
        })?;
        let mut allocated_devices_map = self.allocated_device_ids.write().await;
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
        // let get_preferred_allocation_available = match
        // &plugin_connection.register_request.options {None => false, Some(options) =>
        // options.get_preferred_allocation_available,
        // };

        // if get_preferred_allocation_available {
        //
        // }

        // TODO: support preferred allocation For now, reserve first N devices where N = quantity by
        // adding them to allocated map
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
async fn remove_plugin(
    plugins: Arc<RwLock<HashMap<String, Arc<PluginConnection>>>>,
    resource_name: &str,
    plugin_connection: Arc<PluginConnection>,
) {
    debug!(resource = %resource_name, "Removing plugin");
    let mut lock = plugins.write().await;
    if let Some(old_plugin_connection) = lock.get(resource_name) {
        // TODO: partialEq only checks that reg requests are identical. May also want to check match
        // of other fields.
        if *old_plugin_connection == plugin_connection {
            lock.remove(resource_name);
        }
    }
}

/// Removed all devices of a resource from our shared device map.
async fn remove_resource_devices(devices: Arc<RwLock<DeviceMap>>, resource_name: &str) {
    match devices.write().await.get_mut(resource_name) {
        Some(map) => {
            map.clear();
            trace!(resource = %resource_name,
            "All devices of this resource have been cleared");
        }
        None => trace!(
            resource = %resource_name ,"All devices of this resource were already removed"
        ),
    }
}

fn get_num_from_quantity(q: Quantity) -> anyhow::Result<usize> {
    match q.0.parse::<usize>() {
        Err(e) => Err(anyhow::anyhow!(e)),
        Ok(v) => Ok(v),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{test_utils, PLUGIN_MANGER_SOCKET_NAME};
    use super::*;
    use crate::device_plugin_api::v1beta1::{
        device_plugin_server::{DevicePlugin, DevicePluginServer},
        registration_client, AllocateRequest, AllocateResponse, Device, DevicePluginOptions, Empty,
        ListAndWatchResponse, PreStartContainerRequest, PreStartContainerResponse,
        PreferredAllocationRequest, PreferredAllocationResponse, API_VERSION,
    };
    use futures::Stream;
    use std::pin::Pin;
    use tokio::sync::{mpsc, watch};
    use tonic::{Request, Response, Status};

    /// Mock Device Plugin for testing the DeviceManager Sends a new list of devices to the
    /// DeviceManager whenever it's `devices_receiver` is notified of them on a channel.
    struct MockDevicePlugin {
        // Using watch so the receiver can be cloned and be moved into a spawned thread in
        // ListAndWatch
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
            println!("list_and_watch entered");
            // Create a channel that list_and_watch can periodically send updates to kubelet on
            let (kubelet_update_sender, kubelet_update_receiver) = mpsc::channel(3);
            let mut devices_receiver = self.devices_receiver.clone();
            tokio::spawn(async move {
                while devices_receiver.changed().await.is_ok() {
                    let devices = devices_receiver.borrow().clone();
                    println!(
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
            request: Request<AllocateRequest>,
        ) -> Result<Response<AllocateResponse>, Status> {
            let allocate_request = request.into_inner();
            let container_responses: Vec<ContainerAllocateResponse> = allocate_request
                .container_requests
                .into_iter()
                .map(|_| ContainerAllocateResponse {
                    ..Default::default()
                })
                .collect();
            Ok(Response::new(AllocateResponse {
                container_responses,
            }))
        }

        async fn pre_start_container(
            &self,
            _request: Request<PreStartContainerRequest>,
        ) -> Result<Response<PreStartContainerResponse>, Status> {
            Ok(Response::new(PreStartContainerResponse {}))
        }
    }

    /// Serves the mock DP and returns its socket path
    async fn run_mock_device_plugin(
        devices_receiver: watch::Receiver<Vec<Device>>,
    ) -> anyhow::Result<String> {
        // Device plugin temp socket deleted when it goes out of scope so create it in thread and
        // return with a channel
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn(async move {
            let device_plugin_temp_dir =
                tempfile::tempdir().expect("should be able to create tempdir");
            let socket_name = "gpu-device-plugin.sock";
            let dp_socket = device_plugin_temp_dir
                .path()
                .join(socket_name)
                .to_str()
                .unwrap()
                .to_string();
            tx.send(dp_socket.clone()).unwrap();
            let device_plugin = MockDevicePlugin { devices_receiver };
            let socket =
                grpc_sock::server::Socket::new(&dp_socket).expect("couldn't make dp socket");
            let serv = tonic::transport::Server::builder()
                .add_service(DevicePluginServer::new(device_plugin))
                .serve_with_incoming(socket);
            #[cfg(target_family = "windows")]
            let serv = serv.compat();
            serv.await.expect("Unable to serve mock device plugin");
        });
        Ok(rx.await.unwrap())
    }

    /// Registers the mock DP with the DeviceManager's registration service
    async fn register_mock_device_plugin(
        kubelet_socket: impl AsRef<Path>,
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

    /// Tests e2e flow of kicked off by a mock DP registering with the DeviceManager DeviceManager
    /// should call ListAndWatch on the DP, update it's devices registry with the DP's devices, and
    /// instruct it's NodePatcher to patch the node status with the new DP resources.
    #[tokio::test]
    async fn do_device_manager_test() {
        // There doesn't seem to be a way to use the same temp dir for manager and mock dp due to
        // being able to pass the temp dir reference to multiple threads Instead, create a temp dir
        // for the DP manager and the mock DP
        let manager_temp_dir = tempfile::tempdir().expect("should be able to create tempdir");

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
            health: test_utils::UNHEALTHY.to_string(),
            topology: None,
        };

        // Start the mock device plugin without any devices
        let devices: Vec<Device> = Vec::new();
        let (devices_sender, devices_receiver) = watch::channel(devices);

        // Run the mock device plugin
        let dp_socket = run_mock_device_plugin(devices_receiver).await.unwrap();

        // Name of "this node" that should be patched with Device Plugin resources
        let test_node_name = "test_node";
        // Create and run a mock Kubernetes API service and get a Kubernetes client
        let (client, _mock_service_task) =
            test_utils::create_mock_kube_service(test_node_name).await;

        // Create and serve a DeviceManager
        let device_manager = Arc::new(DeviceManager::new(
            manager_temp_dir.path(),
            client,
            test_node_name,
        ));
        let devices = device_manager.devices.clone();
        let _manager_task = tokio::task::spawn(async move {
            super::super::serve_device_registry(device_manager)
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Register the mock device plugin with the DeviceManager's Registration service
        let dp_resource_name = "example.com/mock-device-plugin";
        register_mock_device_plugin(
            manager_temp_dir.path().join(PLUGIN_MANGER_SOCKET_NAME),
            &dp_socket,
            dp_resource_name,
        )
        .await
        .unwrap();

        // Make DP report 2 healthy and 1 unhealthy device
        devices_sender.send(vec![d1, d2, d3]).unwrap();

        let mut x: i8 = 0;
        let mut num_devices: i8 = 0;
        while x < 3 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            // Assert that there are 3 devices in the map now
            if let Some(resource_devices_map) = devices.read().await.get(dp_resource_name) {
                if resource_devices_map.len() == 3 {
                    num_devices = 3;
                    break;
                }
            }
            x += 1;
        }
        assert_eq!(num_devices, 3);
    }

    fn build_container_allocate_info(
        devices_i_ds: Vec<String>,
        container_name: &str,
    ) -> ContainerAllocateInfo {
        let container_allocate_request = ContainerAllocateRequest { devices_i_ds };
        ContainerAllocateInfo {
            container_allocate_request,
            container_name: container_name.to_string(),
        }
    }

    fn create_device_manager(node_name: &str) -> DeviceManager {
        let client = test_utils::mock_client();
        DeviceManager::new_with_default_path(client, node_name)
    }

    #[test]
    fn test_get_num_from_quantity() {
        assert_eq!(get_num_from_quantity(Quantity("2".to_string())).unwrap(), 2);
    }

    // Test that when a pod requests resources that are not device plugins that no allocate calls
    // are made
    #[tokio::test]
    async fn test_do_allocate_dne() {
        let resource_name = "example.com/other-extended-resource";
        let container_name = "containerA";
        let mut cont_resource_reqs = HashMap::new();
        cont_resource_reqs.insert(resource_name.to_string(), Quantity("2".to_string()));
        let mut pod_resource_req = HashMap::new();
        pod_resource_req.insert(container_name.to_string(), cont_resource_reqs);
        let dm = create_device_manager("some_node");
        // Note: device manager is initialized with an empty devices map, so no resource
        // "example.com/other-extended-resource" will be found
        dm.do_allocate("pod_uid", pod_resource_req).await.unwrap();
        // Allocate should not be called
        assert_eq!(dm.get_pod_allocate_responses("pod_uid").unwrap().len(), 0);
    }

    // Pod with 2 Containers each requesting 2 devices of the same resource
    #[tokio::test]
    async fn test_do_allocate_for_pod() {
        let resource_name = "resource_name";
        let cont_a_device_ids = vec!["id1".to_string(), "id2".to_string()];
        let cont_a_alloc_info = build_container_allocate_info(cont_a_device_ids, "containerA");
        let cont_b_device_ids = vec!["id3".to_string(), "id4".to_string()];
        let cont_b_alloc_info = build_container_allocate_info(cont_b_device_ids, "containerB");
        let mut all_reqs = HashMap::new();
        all_reqs.insert(
            resource_name.to_string(),
            vec![cont_a_alloc_info, cont_b_alloc_info],
        );
        let (_devices_sender, devices_receiver) = watch::channel(Vec::new());

        let dp_socket = run_mock_device_plugin(devices_receiver).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let dm = create_device_manager("some_node");
        dm.create_plugin_connection(RegisterRequest {
            endpoint: dp_socket,
            resource_name: resource_name.to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
        dm.do_allocate_for_pod("pod_uid", all_reqs).await.unwrap();
        // Assert that two responses were stored in `PodDevices`, one for each
        // ContainerAllocateRequest
        assert_eq!(dm.get_pod_allocate_responses("pod_uid").unwrap().len(), 2);
    }
}
