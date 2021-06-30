use super::DeviceMap;
use crate::device_plugin_api::v1beta1::{
    device_plugin_client::DevicePluginClient, AllocateRequest, AllocateResponse, Device, Empty,
    ListAndWatchResponse, RegisterRequest,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tonic::Request;
use tracing::{debug, error, trace};

/// PluginConnection that maps to a single registered device plugin. It is responsible for managing
/// gRPC communications with the device plugin and caching device states reported by the device
/// plugin
pub struct PluginConnection {
    /// Client that is connected to the device plugin
    client: DevicePluginClient<tonic::transport::Channel>,
    /// `RegisterRequest` received when the device plugin registered with the DeviceRegistry
    register_request: RegisterRequest,
}

impl PluginConnection {
    pub fn new(
        client: DevicePluginClient<tonic::transport::Channel>,
        register_request: RegisterRequest,
    ) -> Self {
        PluginConnection {
            client,
            register_request,
        }
    }

    pub async fn start(
        &self,
        devices: Arc<RwLock<DeviceMap>>,
        update_node_status_sender: broadcast::Sender<()>,
    ) {
        match self
            .list_and_watch(devices, update_node_status_sender)
            .await
        {
            Err(e) => {
                error!(error = %e, resource = %self.register_request.resource_name, "ListAndWatch ended unexpectedly for resource")
            }
            Ok(_) => {
                trace!(resource = %self.register_request.resource_name, "Received message to stop ListAndWatch for resource")
            }
        }
    }

    /// Connects to a device plugin's ListAndWatch service. The device plugin updates this client
    /// periodically about changes in the capacity and health of its resource. Upon updates, this
    /// propagates any changes into the `plugins` map and triggers the `NodePatcher` to patch the
    /// node with the latest values.
    async fn list_and_watch(
        &self,
        devices: Arc<RwLock<DeviceMap>>,
        update_node_status_sender: broadcast::Sender<()>,
    ) -> anyhow::Result<()> {
        let mut stream = self
            .client
            .clone()
            .list_and_watch(Request::new(Empty {}))
            .await?
            .into_inner();
        let mut previous_law_devices: HashMap<String, Device> = HashMap::new();
        while let Some(response) = stream.message().await? {
            debug!(resource = %self.register_request.resource_name, "Received ListAndWatch message from this resource");
            if update_devices_map(
                &self.register_request.resource_name,
                devices.clone(),
                &mut previous_law_devices,
                response,
            )
            .await
            {
                if let Err(e) = update_node_status_sender.send(()) {
                    error!(error = %e, "Node status update channel is full");
                }
            }
        }

        Ok(())
    }

    pub async fn allocate(
        &self,
        allocate_request: AllocateRequest,
    ) -> Result<tonic::Response<AllocateResponse>, tonic::Status> {
        self.client
            .clone()
            .allocate(Request::new(allocate_request))
            .await
    }
}

/// This updates the shared device map with the new devices reported by the device plugin. This
/// iterates through the latest devices, comparing them with the previously reported devices and
/// updates the shared device map if:
/// (1) Device modified: DP reporting a previous device with a different health status
/// (2) Device added: DP reporting a new device
/// (3) Device removed: DP is no longer advertising a device
///  If any of the 3 cases occurs, this returns true, signaling that the
/// `NodePatcher` needs to update the Node status with new devices.
async fn update_devices_map(
    resource_name: &str,
    devices: Arc<RwLock<DeviceMap>>,
    previous_law_devices: &mut HashMap<String, Device>,
    response: ListAndWatchResponse,
) -> bool {
    let current_devices = response
        .devices
        .into_iter()
        .map(|device| (device.id.clone(), device))
        .collect::<HashMap<String, Device>>();
    let mut update_node_status = false;

    for device in current_devices.values() {
        // (1) Device modified or already registered
        if let Some(previous_device) = previous_law_devices.get(&device.id) {
            if previous_device.health != device.health {
                add_device_to_map(&mut devices.write().await, resource_name, device);
                update_node_status = true;
            } else if previous_device.topology != device.topology {
                // Currently not using/handling device topology. Simply log the change.
                trace!(resource = %resource_name,
                    "Topology of device changed from {:?} to {:?}",
                    previous_device.topology,
                    device.topology
                );
            }
        // (2) Device added
        } else {
            add_device_to_map(&mut devices.write().await, resource_name, device);
            update_node_status = true;
        }
    }

    // (3) Check if Device removed
    let removed_device_ids: Vec<&String> = previous_law_devices
        .iter()
        .map(|(prev_id, _)| prev_id)
        .filter(|prev_id| !current_devices.contains_key(*prev_id))
        .collect();
    if !removed_device_ids.is_empty() {
        update_node_status = true;
        let mut lock = devices.write().await;
        let map = lock.get_mut(resource_name).unwrap();
        removed_device_ids.iter().for_each(|id| {
            map.remove(*id);
        });
    }

    // Replace previous devices with current devices
    *previous_law_devices = current_devices;

    update_node_status
}

// Adds device to the shared devices map
fn add_device_to_map(
    devices: &mut tokio::sync::RwLockWriteGuard<DeviceMap>,
    resource_name: &str,
    device: &Device,
) {
    match devices.get_mut(resource_name) {
        Some(resource_devices_map) => {
            resource_devices_map.insert(device.id.clone(), device.clone());
        }
        None => {
            let mut resource_devices_map = HashMap::new();
            resource_devices_map.insert(device.id.clone(), device.clone());
            devices.insert(resource_name.to_string(), resource_devices_map);
        }
    }
}

impl PartialEq for PluginConnection {
    fn eq(&self, other: &Self) -> bool {
        self.register_request == other.register_request
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::test_utils::{create_mock_healthy_devices, UNHEALTHY};
    use super::super::HEALTHY;
    use super::*;

    #[tokio::test]
    async fn test_update_devices_map_modified() {
        let (r1_name, r2_name) = ("r1", "r2");
        let devices_map = create_mock_healthy_devices(r1_name, r2_name);
        let mut previous_law_devices = devices_map.read().await.get(r1_name).unwrap().clone();
        let mut devices_vec: Vec<Device> = previous_law_devices.values().cloned().collect();
        // Mark the device offline
        devices_vec[0].health = UNHEALTHY.to_string();
        let unhealthy_id = devices_vec[0].id.clone();
        let response = ListAndWatchResponse {
            devices: devices_vec.clone(),
        };
        let new_previous_law_devices = devices_vec.into_iter().map(|d| (d.id.clone(), d)).collect();
        assert!(
            update_devices_map(
                r1_name,
                devices_map.clone(),
                &mut previous_law_devices,
                response
            )
            .await
        );
        assert_eq!(previous_law_devices, new_previous_law_devices);
        assert_eq!(
            devices_map
                .read()
                .await
                .get(r1_name)
                .unwrap()
                .get(&unhealthy_id)
                .unwrap()
                .health,
            UNHEALTHY
        );
    }

    #[tokio::test]
    async fn test_update_devices_map_added() {
        let (r1_name, r2_name) = ("r1", "r2");
        let devices_map = create_mock_healthy_devices(r1_name, r2_name);
        let mut previous_law_devices = devices_map.read().await.get(r1_name).unwrap().clone();
        let mut devices_vec: Vec<Device> = previous_law_devices.values().cloned().collect();
        // Add another device
        let added_id = format!("{}-id{}", r1_name, 10);
        devices_vec.push(Device {
            id: added_id.clone(),
            health: HEALTHY.to_string(),
            topology: None,
        });
        let response = ListAndWatchResponse {
            devices: devices_vec.clone(),
        };
        let new_previous_law_devices = devices_vec.into_iter().map(|d| (d.id.clone(), d)).collect();
        assert!(
            update_devices_map(
                r1_name,
                devices_map.clone(),
                &mut previous_law_devices,
                response
            )
            .await
        );
        assert_eq!(previous_law_devices, new_previous_law_devices);
        assert_eq!(
            devices_map
                .read()
                .await
                .get(r1_name)
                .unwrap()
                .get(&added_id)
                .unwrap()
                .health,
            HEALTHY
        );
    }

    #[tokio::test]
    async fn test_update_devices_map_removed() {
        let (r1_name, r2_name) = ("r1", "r2");
        let devices_map = create_mock_healthy_devices(r1_name, r2_name);
        let mut previous_law_devices = devices_map.read().await.get(r1_name).unwrap().clone();
        let mut devices_vec: Vec<Device> = previous_law_devices.values().cloned().collect();
        // Remove a device
        let removed_id = devices_vec.pop().unwrap().id;
        let response = ListAndWatchResponse {
            devices: devices_vec.clone(),
        };
        let new_previous_law_devices = devices_vec.into_iter().map(|d| (d.id.clone(), d)).collect();
        assert!(
            update_devices_map(
                r1_name,
                devices_map.clone(),
                &mut previous_law_devices,
                response
            )
            .await
        );
        assert_eq!(previous_law_devices, new_previous_law_devices);
        assert_eq!(
            devices_map
                .read()
                .await
                .get(r1_name)
                .unwrap()
                .get(&removed_id),
            None
        );
    }

    #[tokio::test]
    async fn test_update_devices_map_no_change() {
        let (r1_name, r2_name) = ("r1", "r2");
        let devices_map = create_mock_healthy_devices(r1_name, r2_name);
        let mut previous_law_devices = devices_map.read().await.get(r1_name).unwrap().clone();
        let devices_vec: Vec<Device> = previous_law_devices.values().cloned().collect();
        let response = ListAndWatchResponse {
            devices: devices_vec,
        };
        assert!(
            !update_devices_map(
                r1_name,
                devices_map.clone(),
                &mut previous_law_devices,
                response
            )
            .await
        );
        assert_eq!(
            devices_map.read().await.get(r1_name).unwrap(),
            &previous_law_devices
        );
    }
}
