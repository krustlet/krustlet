pub(crate) mod v1beta1 {
    pub const API_VERSION: &str = "v1beta1";
    tonic::include_proto!("v1beta1");
}
#[cfg(target_os = "linux")]
use super::CONTAINER_PATH;
#[cfg(target_os = "linux")]
use crate::grpc_sock;
use futures::Stream;
use std::path::Path;
use std::pin::Pin;
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use v1beta1::{
    device_plugin_server::{DevicePlugin, DevicePluginServer},
    registration_client, AllocateRequest, AllocateResponse, ContainerAllocateResponse, Device,
    DevicePluginOptions, Empty, ListAndWatchResponse, Mount, PreStartContainerRequest,
    PreStartContainerResponse, PreferredAllocationRequest, PreferredAllocationResponse,
    RegisterRequest, API_VERSION,
};

/// Mock Device Plugin for testing the DeviceManager. Reports its list of devices.
struct MockDevicePlugin {
    // Devices that are advertised
    devices: Vec<Device>,
    // Mount to set on all devices
    volume_mount: String,
}

#[async_trait::async_trait]
impl DevicePlugin for MockDevicePlugin {
    async fn get_device_plugin_options(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<DevicePluginOptions>, Status> {
        unimplemented!();
    }

    type ListAndWatchStream =
        Pin<Box<dyn Stream<Item = Result<ListAndWatchResponse, Status>> + Send + Sync + 'static>>;
    async fn list_and_watch(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::ListAndWatchStream>, Status> {
        println!("list_and_watch entered");
        let (kubelet_update_sender, kubelet_update_receiver) = mpsc::channel(3);
        let devices = self.devices.clone();
        tokio::spawn(async move {
            kubelet_update_sender
                .send(Ok(ListAndWatchResponse { devices }))
                .await
                .unwrap();
            loop {
                // ListAndWatch should not end
                tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;
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
        let mut envs = std::collections::HashMap::new();
        envs.insert("DEVICE_PLUGIN_VAR".to_string(), "foo".to_string());
        let mounts = vec![Mount {
            container_path: CONTAINER_PATH.to_string(),
            host_path: self.volume_mount.clone(),
            read_only: false,
        }];
        let container_responses: Vec<ContainerAllocateResponse> = allocate_request
            .container_requests
            .into_iter()
            .map(|_| ContainerAllocateResponse {
                envs: envs.clone(),
                mounts: mounts.clone(),
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
    devices: Vec<Device>,
    plugin_socket: std::path::PathBuf,
    volume_mount: std::path::PathBuf,
) -> anyhow::Result<()> {
    let device_plugin = MockDevicePlugin {
        devices,
        volume_mount: volume_mount.to_str().unwrap().to_owned(),
    };
    let socket = grpc_sock::server::Socket::new(&plugin_socket)?;
    let serv = tonic::transport::Server::builder()
        .add_service(DevicePluginServer::new(device_plugin))
        .serve_with_incoming(socket);
    #[cfg(target_family = "windows")]
    let serv = serv.compat();
    serv.await.expect("couldnt serve");
    Ok(())
}

/// Registers the mock DP with the DeviceManager's registration service
async fn register_mock_device_plugin(
    kubelet_socket: impl AsRef<Path>,
    plugin_socket: &str,
    resource_name: &str,
) -> anyhow::Result<()> {
    let op = DevicePluginOptions {
        get_preferred_allocation_available: false,
        pre_start_required: false,
    };
    let channel = grpc_sock::client::socket_channel(kubelet_socket).await?;
    let mut registration_client = registration_client::RegistrationClient::new(channel);
    let register_request = tonic::Request::new(RegisterRequest {
        version: API_VERSION.into(),
        endpoint: plugin_socket.to_string(),
        resource_name: resource_name.to_string(),
        options: Some(op),
    });
    registration_client.register(register_request).await?;
    Ok(())
}

fn get_mock_devices() -> Vec<Device> {
    let d1 = Device {
        id: "d1".to_string(),
        health: "Healthy".to_string(),
        topology: None,
    };
    let d2 = Device {
        id: "d2".to_string(),
        health: "Healthy".to_string(),
        topology: None,
    };

    vec![d1, d2]
}

pub async fn launch_device_plugin(
    resource_name: &str,
    resource_endpoint: &str,
    volume_mount: impl AsRef<Path>,
) -> anyhow::Result<()> {
    println!("launching DP test");
    // Create socket for device plugin in the default $HOME/.krustlet/device_plugins directory
    let krustlet_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Unable to get home directory"))?
        .join(".krustlet");
    let kubelet_socket = krustlet_dir.join("device_plugins").join("kubelet.sock");
    let dp_socket = krustlet_dir.join("device_plugins").join(resource_endpoint);
    tokio::fs::remove_file(&dp_socket).await.ok();
    let dp_socket_clone = dp_socket.clone();
    let volume_mount_string = volume_mount.as_ref().to_owned();
    tokio::spawn(async move {
        run_mock_device_plugin(get_mock_devices(), dp_socket, volume_mount_string)
            .await
            .unwrap();
    });

    // Wait for device plugin to be served
    let time = std::time::Instant::now();
    loop {
        if time.elapsed().as_secs() > 1 {
            return Err(anyhow::anyhow!("Could not connect to device plugin"));
        }
        if grpc_sock::client::socket_channel(dp_socket_clone.clone())
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    register_mock_device_plugin(
        kubelet_socket,
        dp_socket_clone.to_str().unwrap(),
        resource_name,
    )
    .await?;
    Ok(())
}
