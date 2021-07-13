#[path = "../socket_server.rs"]
pub mod socket_server;
pub(crate) mod v1beta1 {
    pub const API_VERSION: &str = "v1beta1";
    tonic::include_proto!("v1beta1");
}
use v1beta1::{
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