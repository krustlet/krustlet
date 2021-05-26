
use super::{DeviceMap, HEALTHY, UNHEALTHY};
use kube::api::{Api, PatchParams};
use k8s_openapi::api::core::v1::{Node, NodeStatus};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

/// NodePatcher updates the Node status with the latest device information.
#[derive(Clone)]
pub struct NodeStatusPatcher {
    devices: Arc<Mutex<DeviceMap>>,
    // Broadcast sender so clonable
    update_node_status_sender: broadcast::Sender<()>,
}

impl NodeStatusPatcher {
    pub fn new(devices: Arc<Mutex<DeviceMap>>, update_node_status_sender: broadcast::Sender<()>) -> Self {
        NodeStatusPatcher {devices, update_node_status_sender}
    }

    async fn get_node_status_patch(
        &self,
    ) -> NodeStatus {
        let devices = self.devices.lock().unwrap();
        let capacity: BTreeMap<String, Quantity> = devices.iter().map(|(resource_name, resource_devices)| (resource_name.clone(), Quantity(resource_devices.len().to_string()))).collect();
        let allocatable: BTreeMap<String, Quantity> = devices.iter().map(|(resource_name, resource_devices)| {
            let healthy_count: usize = resource_devices.iter().filter(|(_, dev)| dev.health == HEALTHY).map(|(_, _)| 1).sum();
            (resource_name.clone(), Quantity(healthy_count.to_string()))
        }).collect();
        NodeStatus {
            capacity: Some(capacity),
            allocatable: Some(allocatable),
            ..Default::default()
        }
    }

    async fn do_node_status_patch(
        &self,
        status: NodeStatus,
        node_name: &str,
        client: &kube::Client,
    ) -> anyhow::Result<()> {
        let node_client: Api<Node> = Api::all(client.clone());
        let _node = node_client
            .patch_status(
                node_name,
                &PatchParams::default(),
                &kube::api::Patch::Strategic(status),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Unable to patch node status: {}", e))?;
        Ok(())
    }

    pub async fn listen_and_patch(
        self,
        node_name: String,
        client: kube::Client,
    ) -> anyhow::Result<()> {
        // Forever hold lock on the status update receiver
        let mut receiver = self.update_node_status_sender.subscribe();
        println!("entered listen_and_patch");
        loop {
            println!("listen_and_patch loop");
            match receiver.recv().await {
                Err(e) => {
                    error!("Channel closed by senders");
                    // TODO: bubble up error
                },
                Ok(_) => {
                    // Grab status values
                    let status_patch = self.get_node_status_patch().await;
                    // Do patch 
                    self.do_node_status_patch(status_patch, &node_name, &client).await?;
                }
            }
        }
        // TODO add channel for termination?
        Ok(())
    }
}


#[cfg(test)]
mod node_patcher_tests {
    use super::super::{EndpointDevicesMap, UNHEALTHY};
    use super::super::manager::manager_tests::create_mock_kube_service;
    use super::*;
    use crate::device_plugin_api::v1beta1::Device;

    fn create_mock_devices(r1_name: &str, r2_name: &str) -> Arc<Mutex<DeviceMap>> {
        let r1_devices: EndpointDevicesMap = [
            ("r1-id1".to_string(), Device{id: "r1-id1".to_string(), health: HEALTHY.to_string(), topology: None}), 
            ("r1-id2".to_string(), Device{id: "r1-id2".to_string(), health: HEALTHY.to_string(), topology: None}),
            ("r1-id3".to_string(), Device{id: "r1-id3".to_string(), health: UNHEALTHY.to_string(), topology: None})
            ].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let r2_devices: EndpointDevicesMap = [
            ("r2-id1".to_string(), Device{id: "r2-id1".to_string(), health: HEALTHY.to_string(), topology: None}), 
            ("r2-id2".to_string(), Device{id: "r2-id2".to_string(), health: HEALTHY.to_string(), topology: None})
            ].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let device_map: DeviceMap = [(r1_name.to_string(), r1_devices), (r2_name.to_string(), r2_devices)].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        Arc::new(Mutex::new(device_map))
    }

    #[tokio::test]
    async fn test_do_node_status_patch() {
        let devices = create_mock_devices("r1", "r2");
        let empty_node_status = NodeStatus {
            capacity: None,
            allocatable: None,
            ..Default::default()
        };
        let (update_node_status_sender, _rx) = broadcast::channel(2);
        let node_status_patcher = NodeStatusPatcher {devices, update_node_status_sender};
        let node_name = "test_node";
        let (client, _) = create_mock_kube_service(node_name).await;
        node_status_patcher.do_node_status_patch(empty_node_status, node_name, &client).await.unwrap();
    }

    #[tokio::test]
    async fn test_get_node_status_patch() {
        let r1_name = "r1";
        let r2_name = "r2";
        let devices = create_mock_devices(r1_name, r2_name);
        let (update_node_status_sender, _rx) = broadcast::channel(2);
        let node_status_patcher = NodeStatusPatcher {devices, update_node_status_sender};
        let status = node_status_patcher.get_node_status_patch().await;
        // Check that both resources listed under allocatable and only healthy devices are counted
        let allocatable = status.allocatable.unwrap();
        assert_eq!(allocatable.len(), 2);
        assert_eq!(allocatable.get(r1_name).unwrap(), &Quantity("2".to_string()));
        assert_eq!(allocatable.get(r2_name).unwrap(), &Quantity("2".to_string()));

        // Check that both resources listed under capacity and both healthy and unhealthy devices are counted
        let capacity = status.capacity.unwrap();
        assert_eq!(capacity.len(), 2);
        assert_eq!(capacity.get(r1_name).unwrap(), &Quantity("3".to_string()));
        assert_eq!(capacity.get(r2_name).unwrap(), &Quantity("2".to_string()));

    }
}