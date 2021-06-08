//! The Kubelet device plugin manager. Consists of a `DeviceRegistry` that hosts a registration service for device plugins, a `DeviceManager` that maintains a device plugin client for each registered device plugin, a `NodePatcher` that patches the Node status with the extended resources advertised by device plugins, and a `PodDevices` that maintains a list of Pods that are actively using allocated resources.
pub mod manager;
pub(crate) mod node_patcher;
pub(crate) mod plugin_connection;
pub(crate) mod pod_devices;
pub mod resources;
use crate::device_plugin_api::v1beta1::{
    registration_server::{Registration, RegistrationServer},
    Device, Empty, RegisterRequest,
};
use crate::grpc_sock;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
pub use manager::DeviceManager;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::task;
#[cfg(target_family = "windows")]
use tokio_compat_02::FutureExt;
use tonic::transport::Server;

const PLUGIN_MANGER_SOCKET_NAME: &str = "kubelet.sock";

/// `DeviceIdMap` contains the device Ids of all the devices advertised by device plugins.
/// Key is resource name.
type DeviceIdMap = HashMap<String, PluginDeviceIds>;

/// `PluginDeviceIds` contains the IDs of all the devices advertised by a single device plugin
type PluginDeviceIds = HashSet<String>;

/// `DeviceMap` contains all the devices advertised by all device plugins. Key is resource name.
type DeviceMap = HashMap<String, PluginDevicesMap>;

/// `PluginDevicesMap` contains all of the devices advertised by a single device plugin. Key is device ID.
type PluginDevicesMap = HashMap<String, Device>;

/// Map of resources requested by a Container. Key is resource name and value is requested quantity of the resource
type ContainerResourceRequests = HashMap<String, Quantity>;

/// Map of resources requested by the Containers of a Pod. Key is container name and value is the Container's resource requests
pub type PodResourceRequests = HashMap<String, ContainerResourceRequests>;

/// Healthy means the device is allocatable (whether already allocated or not)
const HEALTHY: &str = "Healthy";

/// Unhealthy means the device is not allocatable
const UNHEALTHY: &str = "Unhealthy";

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
        let register_request = request.into_inner();
        // Validate
        self.device_manager
            .validate(&register_request)
            .await
            .map_err(|e| tonic::Status::new(tonic::Code::InvalidArgument, format!("{}", e)))?;
        // Create a list and watch connection with the device plugin
        // TODO: should the manager keep track of threads?
        self.device_manager
            .create_plugin_connection(register_request)
            .await
            .map_err(|e| tonic::Status::new(tonic::Code::NotFound, format!("{}", e)))?;
        Ok(tonic::Response::new(Empty {}))
    }
}

/// Starts the `DeviceManager` by running its `NodePatcher` and serving the `DeviceRegistry` which hosts the device plugin manager's `Registration` service on the socket
/// specified in the `DeviceManager`.
/// Returns an error if either the `NodePatcher` or `DeviceRegistry` error.
pub async fn serve_device_registry(device_manager: Arc<DeviceManager>) -> anyhow::Result<()> {
    // Create plugin manager if it doesn't exist
    tokio::fs::create_dir_all(&device_manager.plugin_dir).await?;
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
pub mod test_utils {
    use super::*;
    use futures::pin_mut;
    use http::{Request as HttpRequest, Response as HttpResponse};
    use hyper::Body;
    use kube::Client;
    use std::sync::Mutex;
    use tower_test::mock;

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

    pub fn create_mock_healthy_devices(r1_name: &str, r2_name: &str) -> Arc<Mutex<DeviceMap>> {
        let r1_devices: PluginDevicesMap = (0..3)
            .map(|x| Device {
                id: format!("{}-id{}", r1_name, x),
                health: HEALTHY.to_string(),
                topology: None,
            })
            .map(|d| (d.id.clone(), d))
            .collect();

        let r2_devices: PluginDevicesMap = (0..2)
            .map(|x| Device {
                id: format!("{}-id{}", r2_name, x),
                health: HEALTHY.to_string(),
                topology: None,
            })
            .map(|d| (d.id.clone(), d))
            .collect();

        let device_map: DeviceMap = [
            (r1_name.to_string(), r1_devices),
            (r2_name.to_string(), r2_devices),
        ]
        .iter()
        .cloned()
        .collect();

        Arc::new(Mutex::new(device_map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_plugin_api::v1beta1::{
        device_plugin_server::{DevicePlugin, DevicePluginServer},
        registration_client, AllocateRequest, AllocateResponse, DevicePluginOptions, Empty,
        ListAndWatchResponse, PreStartContainerRequest, PreStartContainerResponse,
        PreferredAllocationRequest, PreferredAllocationResponse, API_VERSION,
    };
    use futures::Stream;
    use std::pin::Pin;
    use tokio::sync::{mpsc, watch};
    use tonic::{Request, Response, Status};

    /// Mock Device Plugin for testing the DeviceManager
    /// Sends a new list of devices to the DeviceManager whenever it's `devices_receiver`
    /// is notified of them on a channel.
    struct MockDevicePlugin {
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
        socket_path: impl AsRef<std::path::Path>,
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
        let _device_plugin_task = tokio::task::spawn(async move {
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
        let (client, _mock_service_task) =
            test_utils::create_mock_kube_service(test_node_name).await;

        // Create and serve a DeviceManager
        let device_manager = Arc::new(DeviceManager::new(
            manager_temp_dir.path(),
            client,
            test_node_name,
        ));
        let devices = device_manager.devices.clone();
        let _manager_task = task::spawn(async move {
            serve_device_registry(device_manager).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Register the mock device plugin with the DeviceManager's Registration service
        let dp_resource_name = "example.com/mock-device-plugin";
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
