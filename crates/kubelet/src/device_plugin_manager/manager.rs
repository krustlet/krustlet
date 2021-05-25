//! The Kubelet device plugin manager. Hosts a registration service for device plugins, creates a device plugin client for each registered device plugin, updates node with the extended resources advertised by device plugins.
use crate::device_plugin_api::v1beta1::{
    API_VERSION,
    Device, Empty, RegisterRequest,
    registration_server::{Registration, RegistrationServer},
    device_plugin_client::DevicePluginClient,
};
use crate::grpc_sock;
use super::{DeviceIdMap, DeviceMap, HEALTHY, UNHEALTHY};
use super::node_patcher::{listen_and_patch, NodeStatusPatcher, NodeStatusPatcherImpl};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, watch};
use tokio::task;
use tonic::{transport::Server, Request};
#[cfg(target_family = "windows")]
use tokio_compat_02::FutureExt;
use tracing::{debug, error, trace, warn};

#[cfg(target_family = "unix")]
pub const DEFAULT_PLUGIN_PATH: &str = "/var/lib/kubelet/device_plugins/";
#[cfg(target_family = "windows")]
pub const DEFAULT_PLUGIN_PATH: &str = "c:\\ProgramData\\kubelet\\device_plugins";

const PLUGIN_MANGER_SOCKET_NAME: &str = "kubelet.sock";

/// Endpoint that maps to a single registered device plugin.
/// It is responsible for managing gRPC communications with the device plugin and caching
/// device states reported by the device plugin
pub struct Endpoint {
    pub client: DevicePluginClient<tonic::transport::Channel>,
    pub register_request: RegisterRequest,
}

type PluginMap = Arc<Mutex<HashMap<String,Endpoint>>>;

/// An internal storage plugin registry that implements most the same functionality as the [plugin
/// manager](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/pluginmanager)
/// in kubelet
pub struct DeviceManager {
    /// Registered plugins
    pub plugins: PluginMap,
    /// Directory where the plugin sockets live
    pub plugin_dir: PathBuf,
    /// Device map
    pub devices: Arc<Mutex<DeviceMap>>,
    /// TODO
    pub allocated_device_ids: Arc<Mutex<DeviceIdMap>>,
    /// TODO
    // pub healthy_device_ids: Arc<Mutex<DeviceIdMap>>,
    // pub unhealthy_device_ids: Arc<Mutex<DeviceIdMap>>,
    /// update_node_status_sender notifies the Node patcher to update node status with latest values
    update_node_status_sender: mpsc::Sender<()>,
}

impl DeviceManager {
    /// Returns a new device manager configured with the given device plugin directory path
    pub fn new<P: AsRef<Path>>(plugin_dir: P, update_node_status_sender: mpsc::Sender<()>) -> Self {
        DeviceManager {
            plugin_dir: PathBuf::from(plugin_dir.as_ref()),
            plugins: Arc::new(Mutex::new(HashMap::new())),
            devices: Arc::new(Mutex::new(HashMap::new())),
            allocated_device_ids: Arc::new(Mutex::new(HashMap::new())),
            update_node_status_sender
        }
    }
    /// Returns a new device manager configured with the default `/var/lib/kubelet/device_plugins/` device plugin directory path
    pub fn default(update_node_status_sender: mpsc::Sender<()>) -> Self {
        DeviceManager {
            plugin_dir: PathBuf::from(DEFAULT_PLUGIN_PATH),
            plugins: Arc::new(Mutex::new(HashMap::new())),
            devices: Arc::new(Mutex::new(HashMap::new())),
            allocated_device_ids: Arc::new(Mutex::new(HashMap::new())),
            // healthy_device_ids,
            update_node_status_sender
            // unhealthy_device_ids: Arc::new(Mutex::new(HashMap::new()))
        }
    }

    /// Adds the plugin to our HashMap
    fn add_plugin(&self, endpoint: Endpoint) {
        let mut lock = self.plugins.lock().unwrap();
        // let (connection_directive_sender, _) = watch::channel(ConnectionDirective::CONTINUE);
        // let plugin_entry = PluginEntry { 
        //     endpoint,
        //     connection_directive_sender 
        // };
        lock.insert(
            endpoint.register_request.resource_name.clone(),
            endpoint,
        );
    }

    /// Removes the plugin from our HashMap
    async fn remove_plugin(&self, resource_name: &str) {
        let mut lock = self.plugins.lock().unwrap();
        lock.remove(
            resource_name
        );
    }

    
    /// Validates the given plugin info gathered from a discovered plugin, returning an error with
    /// additional information if it is not valid. This will validate 3 specific things (should
    /// answer YES to all of these):
    /// 1. Does this manager support the device plugin version? Currently only accepting `API_VERSION`.
    /// TODO: determine whether can support all versions prior to current `API_VERSION`.
    /// 2. Is the plugin name available? 2a. If the name is already registered, is the endpoint the
    ///    exact same? If it is, we allow it to reregister
    async fn validate(&self, register_request: &RegisterRequest) -> Result<(), tonic::Status>  {
        trace!(
            "Starting validation for plugin {:?} discovered at path {}",
            register_request.resource_name,
            register_request.endpoint
        );
        // Validate that version matches the Device Plugin API version
        if register_request.version != API_VERSION {
            return Err(tonic::Status::new(tonic::Code::Unimplemented, format!("kubelet doesn't support version Device Plugin {}",
                API_VERSION)
            ));
        };
        
        // TODO: validate that plugin has proper extended resource name
        // https://github.com/kubernetes/kubernetes/blob/ea0764452222146c47ec826977f49d7001b0ea8c/pkg/kubelet/cm/devicemanager/manager.go#L309

        // TODO: validate that endpoint is in proper directory

        self.validate_is_unique(register_request).await?;

        Ok(())        
    }

    /// Validates if the plugin is unique (meaning it doesn't exist among the plugins map).
    /// If there is an active plugin registered with this name, returns error.
    async fn validate_is_unique(
        &self,
        register_request: &RegisterRequest
    ) -> Result<(), tonic::Status>  {
        let plugins = self.plugins.lock().unwrap();

        if let Some(previous_plugin_entry) = plugins.get(&register_request.resource_name) {
            // TODO: check if plugin is active
            return Err(tonic::Status::new(tonic::Code::AlreadyExists, format!("Device Plugin with resource name {} already registered", register_request.resource_name)));
        }

        Ok(())
    }

    // TODO rename or break into multiple functions
    async fn create_endpoint(&self, register_request: &RegisterRequest)  -> anyhow::Result<()>  {
        trace!(
            "Connecting to plugin at {:?} for ListAndWatch",
            register_request.endpoint
        );
        let chan = grpc_sock::client::socket_channel(register_request.endpoint.clone()).await?;
        let client = DevicePluginClient::new(chan);

        // Clone structures for ListAndWatch thread
        let mut list_and_watch_client  = client.clone();
        let list_and_watch_resource_name = register_request.resource_name.clone();
        let all_devices = self.devices.clone();
        // let healthy_devices = self.healthy_device_ids.clone();
        let update_node_status_sender = self.update_node_status_sender.clone();

        // TODO: make options an enum?
        let success: i8 = 0;
        let error: i8 = 1;
        let (successful_connection_sender, successful_connection_receiver): (tokio::sync::oneshot::Sender<i8>, tokio::sync::oneshot::Receiver<i8>) = tokio::sync::oneshot::channel();
        
        // TODO: decide whether to join all spawned ListAndWatch threads
        tokio::spawn(async move {
            match list_and_watch_client.list_and_watch(Request::new(Empty {})).await {
                Err(e) => {
                    error!("could not call ListAndWatch on device plugin with resource name {:?} with error {}", list_and_watch_resource_name, e);
                    successful_connection_sender.send(error).unwrap();
                }
                Ok(stream_wrapped) => {
                    successful_connection_sender.send(success).unwrap();
                    let mut stream = stream_wrapped.into_inner();
                    let mut previous_endpoint_devices: HashMap<String, Device> = HashMap::new();
                    while let Some(response) = stream.message().await.unwrap() {
                        let current_devices = response.devices.iter().map(|device| (device.id.clone(), device.clone())).collect::<HashMap<String,Device>>();
                        let mut update_node_status = false;
                        // Iterate through the list of devices, updating the Node status if 
                        // (1) Device modified: DP reporting a previous device with a different health status 
                        // (2) Device added: DP reporting a new device 
                        // (3) Device removed: DP is no longer advertising a device
                        current_devices.iter().for_each(|(_, device)| { 
                            // (1) Device modified or already registered
                            if let Some(previous_device) = previous_endpoint_devices.get(&device.id) {
                                if previous_device.health != device.health {
                                   all_devices.lock().unwrap().get_mut(&list_and_watch_resource_name).unwrap().insert(device.id.clone(), device.clone());
                                   if device.health == HEALTHY {
                                       // Add device to healthy map
                                    //    healthy_devices.lock().unwrap().get_mut(&list_and_watch_resource_name).unwrap().insert(device.id.clone());
                                   }
                                   update_node_status = true;
                                } else if previous_device.topology != device.topology {
                                    // TODO: how to handle this
                                    error!("device topology changed");
                                }
                            // (2) Device added
                            } else {
                                let mut all_devices_map = all_devices.lock().unwrap();
                                match all_devices_map.get_mut(&list_and_watch_resource_name) {
                                    Some(resource_devices_map) => { resource_devices_map.insert(device.id.clone(), device.clone()); },
                                    None => {
                                        let mut resource_devices_map = HashMap::new();
                                        resource_devices_map.insert(device.id.clone(), device.clone());
                                        all_devices_map.insert(list_and_watch_resource_name.clone(), resource_devices_map);
                                    }
                                }
                                if device.health == HEALTHY {
                                    // Add device to healthy map
                                    // healthy_devices.lock().unwrap().get_mut(&list_and_watch_resource_name).unwrap().insert(device.id.clone());
                                }
                                update_node_status = true;
                            }
                        });

                        // (3) Check if Device removed
                        previous_endpoint_devices.iter().for_each(|(_, previous_device)| {
                            if !response.devices.contains(previous_device) {
                                if previous_device.health == HEALTHY {
                                    // Remove device from healthy map
                                    // healthy_devices.lock().unwrap().get_mut(&list_and_watch_resource_name).unwrap().remove(&previous_device.id);
                                }
                                // TODO: how to handle already allocated devices? Pretty sure K8s lets them keep running but what about the allocated_device map?
                                all_devices.lock().unwrap().get_mut(&list_and_watch_resource_name).unwrap().remove(&previous_device.id);
                                update_node_status = true;
                            }
                        });

                        // Replace previous devices with current devices
                        previous_endpoint_devices = current_devices;

                        if update_node_status {
                            // TODO handle error -- maybe channel is full
                            update_node_status_sender.send(()).await.unwrap();
                        }
                    }
                }
            }
            
        });

        // Only add device plugin to map if successful ListAndWatch call
        if successful_connection_receiver.await.unwrap() == success {
            let endpoint = Endpoint{client, register_request: register_request.clone()};
            self.add_plugin(endpoint);
        } else {
            return Err(anyhow::Error::msg(format!("could not call ListAndWatch on device plugin at socket {:?}", register_request.endpoint)));
        }
        
        Ok(())
    }

}


#[async_trait::async_trait]
impl Registration for DeviceManager {
    async fn register(
        &self,
        request: tonic::Request<RegisterRequest>,
    ) -> Result<tonic::Response<Empty>, tonic::Status> {
        let register_request = request.get_ref();
        // Validate
        self.validate(register_request).await.map_err(|e| tonic::Status::new(tonic::Code::InvalidArgument, format!("{}", e)))?;
        // Create a list and watch connection with the device plugin
        // TODO: should the manager keep track of threads?
        self.create_endpoint(register_request).await.map_err(|e| tonic::Status::new(tonic::Code::NotFound, format!("{}", e)))?;
        Ok(tonic::Response::new(Empty {}))
    }
        
}

pub async fn serve_device_manager(device_manager: DeviceManager, update_node_status_receiver: mpsc::Receiver<()>, client: &kube::Client, node_name: &str) -> anyhow::Result<()>  {
    let node_patcher = NodeStatusPatcherImpl{ devices: device_manager.devices.clone()};
    internal_serve_device_manager(device_manager, node_patcher, update_node_status_receiver, client, node_name).await
}

/// TODO
pub async fn internal_serve_device_manager(device_manager: DeviceManager, node_patcher: impl NodeStatusPatcher, update_node_status_receiver: mpsc::Receiver<()>, client: &kube::Client, node_name: &str) -> anyhow::Result<()>  {
     // TODO determin if need to create socket (and delete any previous ones)
    let manager_socket = device_manager.plugin_dir.join(PLUGIN_MANGER_SOCKET_NAME);
    let socket = grpc_sock::server::Socket::new(&manager_socket).expect("couldn't make manager socket");
    
    // Clone arguments for listen_and_patch thread
    let node_patcher_task_client = client.clone();
    let node_patcher_task_node_name = node_name.to_string();
    let node_patcher_task = task::spawn(async move {
        listen_and_patch(update_node_status_receiver, node_patcher_task_node_name, node_patcher_task_client, node_patcher).await.unwrap();
    });
    println!("before serve");
    let device_manager_task = task::spawn(async {
        let serv = Server::builder()
        .add_service(RegistrationServer::new(device_manager))
        .serve_with_incoming(socket);
        #[cfg(target_family = "windows")]
        let serv = serv.compat();
        serv.await.expect("Unable to serve device plugin manager");
    });
    tokio::try_join!(node_patcher_task, device_manager_task).map_err(anyhow::Error::from)?;
    Ok(())
}


#[cfg(test)]
mod manager_tests {
    use super::super::node_patcher::MockNodeStatusPatcher;
    use super::*;
    use crate::device_plugin_api::v1beta1::{
        device_plugin_server::{DevicePlugin, DevicePluginServer}, AllocateRequest, AllocateResponse, DevicePluginOptions,
    DeviceSpec, Empty, ListAndWatchResponse, Mount, PreStartContainerRequest, PreferredAllocationRequest, PreferredAllocationResponse,
    PreStartContainerResponse, registration_client,
    };
    use futures::{pin_mut, Stream, StreamExt};
    use http::{Request as HttpRequest, Response as HttpResponse};
    use hyper::Body;
    use kube::{Client, Service};
    use std::pin::Pin;
    use tokio::sync::watch;
    use tempfile::Builder;
    use tonic::{Code, Request, Response, Status};
    use tower_test::{mock, mock::Handle};

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

        type ListAndWatchStream =  Pin<Box<dyn Stream<Item = Result<ListAndWatchResponse, Status>> + Send + Sync + 'static>>;
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
                    println!("list_and_watch received new devices [{:?}] to send", devices);
                    kubelet_update_sender.send(Ok(ListAndWatchResponse{devices})).await.unwrap();
                }
            });
            Ok(Response::new(Box::pin(
                tokio_stream::wrappers::ReceiverStream::new(kubelet_update_receiver),
            )))
        }

        async fn get_preferred_allocation(
            &self,
            request: Request<PreferredAllocationRequest>,
        ) -> Result<Response<PreferredAllocationResponse>, Status> {
            unimplemented!();
        }

        async fn allocate(
            &self,
            requests: Request<AllocateRequest>,
        ) -> Result<Response<AllocateResponse>, Status> {
            unimplemented!();
        }

        async fn pre_start_container(
            &self,
            _request: Request<PreStartContainerRequest>,
        ) -> Result<Response<PreStartContainerResponse>, Status> {
            Ok(Response::new(PreStartContainerResponse{}))
        }
    }

    /// Serves the mock DP 
    async fn run_mock_device_plugin(socket_path: impl AsRef<Path>, devices_receiver: watch::Receiver<Vec<Device>>) -> anyhow::Result<()> {
        let device_plugin = MockDevicePlugin{devices_receiver};
        let socket = grpc_sock::server::Socket::new(&socket_path).expect("couldnt make dp socket");
        println!("after creating DP socket");
        let serv = Server::builder()
        .add_service(DevicePluginServer::new(device_plugin))
        .serve_with_incoming(socket);
        #[cfg(target_family = "windows")]
        let serv = serv.compat();
        serv.await.expect("Unable to serve mock device plugin");
        Ok(())
    }

    /// Registers the mock DP with the DeviceManager's registration service
    async fn register_mock_device_plugin(kubelet_socket: String, dp_socket: &str, dp_resource_name: &str) -> anyhow::Result<()> { 
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
            .await.unwrap();
        Ok(())
    }

    /// Creates a mock kubernetes API service that the NodePatcher calls to when device plugin resources 
    /// need to be updated in the Node status. 
    /// It verifies the request and always returns a fake Node.
    /// Returns a client that will reference this mock service and the task the service is running on.
    /// TODO: Decide whether to test node status
    async fn create_mock_kube_service(node_name: &str) -> (Client, tokio::task::JoinHandle<()>) {
        // Mock client as inspired by this thread on kube-rs crate: https://github.com/clux/kube-rs/issues/429
        let (mock_service, handle) = mock::pair::<HttpRequest<Body>, HttpResponse<Body>>();
        let service = Service::new(mock_service);
        let node_name = node_name.to_string();
        let spawned = tokio::spawn(async move {
            pin_mut!(handle);
            let (request, send) = handle.next_request().await.expect("service not called");
            assert_eq!(request.method(), http::Method::PATCH);
            assert_eq!(request.uri().to_string(), format!("/api/v1/nodes/{}/status?", node_name));
            let node: k8s_openapi::api::core::v1::Node = serde_json::from_value(serde_json::json!({
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
        let client = Client::new(service);
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
        let dp_socket = device_plugin_temp_dir.path().join(socket_name).to_str()
        .unwrap()
        .to_string();
        let manager_socket = manager_temp_dir
            .path()
            .join(PLUGIN_MANGER_SOCKET_NAME)
            .to_str()
            .unwrap()
            .to_string();

        // Name of "this node" that should be patched with Device Plugin resources
        let test_node_name = "test_node";

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
        let device_plugin_task = task::spawn( async move {
            run_mock_device_plugin(device_plugin_temp_dir.path().join(socket_name), devices_receiver).await.unwrap();
        });

        // Create and run a mock Kubernetes API service and get a Kubernetes client
        let (client, mock_service_task) = create_mock_kube_service(test_node_name).await;

        // Create and serve a DeviceManager
        let (update_node_status_sender, update_node_status_receiver) = mpsc::channel(2);
        let manager = DeviceManager::new(manager_temp_dir.path().clone(), update_node_status_sender);
        let devices = manager.devices.clone();
        let manager_task = task::spawn( async move {
            serve_device_manager(manager, update_node_status_receiver, &client, test_node_name).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    
        // Register the mock device plugin with the DeviceManager's Registration service
        let dp_resource_name = "mock-device-plugin";
        register_mock_device_plugin(manager_socket.to_string(), &dp_socket, dp_resource_name).await.unwrap();
        
        // Make DP report 2 healthy and 1 unhealthy device
        devices_sender.send(vec![d1, d2, d3]).unwrap();

        let mut x: i8 = 0;
        let mut num_devices: i8 = 0;
        while x < 3 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            // Assert that there are 3 devices in the map now
            if let Some(resource_devices_map) = devices.lock().unwrap().get(dp_resource_name){
                if resource_devices_map.len() == 3 {
                    num_devices = 3;
                    break;
                }
            }
            x+=1;
        }
        assert_eq!(num_devices, 3);

        // tokio::join!(device_plugin_task, mock_service_task, manager_task);
    }



}