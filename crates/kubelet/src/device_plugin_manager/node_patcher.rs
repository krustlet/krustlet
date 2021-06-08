use super::{DeviceMap, HEALTHY};
use k8s_openapi::api::core::v1::{Node, NodeStatus};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::{Api, PatchParams};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::error;

/// NodePatcher updates the Node status with the latest device information.
#[derive(Clone)]
pub struct NodeStatusPatcher {
    node_name: String,
    devices: Arc<Mutex<DeviceMap>>,
    // Broadcast sender so clonable
    update_node_status_sender: broadcast::Sender<()>,
    client: kube::Client,
}

impl NodeStatusPatcher {
    pub fn new(
        node_name: &str,
        devices: Arc<Mutex<DeviceMap>>,
        update_node_status_sender: broadcast::Sender<()>,
        client: kube::Client,
    ) -> Self {
        NodeStatusPatcher {
            node_name: node_name.to_string(),
            devices,
            update_node_status_sender,
            client,
        }
    }

    async fn get_node_status_patch(&self) -> NodeStatus {
        let devices = self.devices.lock().unwrap();
        let capacity: BTreeMap<String, Quantity> = devices
            .iter()
            .map(|(resource_name, resource_devices)| {
                (
                    resource_name.clone(),
                    Quantity(resource_devices.len().to_string()),
                )
            })
            .collect();
        let allocatable: BTreeMap<String, Quantity> = devices
            .iter()
            .map(|(resource_name, resource_devices)| {
                let healthy_count: usize = resource_devices
                    .iter()
                    .filter(|(_, dev)| dev.health == HEALTHY)
                    .map(|(_, _)| 1)
                    .sum();
                (resource_name.clone(), Quantity(healthy_count.to_string()))
            })
            .collect();
        NodeStatus {
            capacity: Some(capacity),
            allocatable: Some(allocatable),
            ..Default::default()
        }
    }

    async fn do_node_status_patch(&self, status: NodeStatus) -> anyhow::Result<()> {
        let node_client: Api<Node> = Api::all(self.client.clone());
        let _node = node_client
            .patch_status(
                &self.node_name,
                &PatchParams::default(),
                &kube::api::Patch::Strategic(status),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Unable to patch node status: {}", e))?;
        Ok(())
    }

    pub async fn listen_and_patch(self) -> anyhow::Result<()> {
        // Forever hold lock on the status update receiver
        let mut receiver = self.update_node_status_sender.subscribe();
        loop {
            match receiver.recv().await {
                Err(_e) => {
                    error!("Channel closed by senders");
                    // TODO: bubble up error
                }
                Ok(_) => {
                    // Grab status values
                    let status_patch = self.get_node_status_patch().await;
                    // Do patch
                    self.do_node_status_patch(status_patch).await?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_utils::{create_mock_healthy_devices, create_mock_kube_service};
    use super::super::UNHEALTHY;
    use super::*;

    #[tokio::test]
    async fn test_do_node_status_patch() {
        let devices = create_mock_healthy_devices("r1", "r2");
        devices
            .lock()
            .unwrap()
            .get_mut("r1")
            .unwrap()
            .get_mut("r1-id1")
            .unwrap()
            .health = UNHEALTHY.to_string();
        let empty_node_status = NodeStatus {
            capacity: None,
            allocatable: None,
            ..Default::default()
        };
        let (update_node_status_sender, _rx) = broadcast::channel(2);

        // Create and run a mock Kubernetes API service and get a Kubernetes client
        let (client, _mock_service_task) = create_mock_kube_service("test_node").await;
        let node_name = "test_node";
        let node_status_patcher =
            NodeStatusPatcher::new(node_name, devices, update_node_status_sender, client);
        node_status_patcher
            .do_node_status_patch(empty_node_status)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_get_node_status_patch() {
        let r1_name = "r1";
        let r2_name = "r2";
        let devices = create_mock_healthy_devices(r1_name, r2_name);
        devices
            .lock()
            .unwrap()
            .get_mut("r1")
            .unwrap()
            .get_mut("r1-id1")
            .unwrap()
            .health = UNHEALTHY.to_string();
        let (update_node_status_sender, _rx) = broadcast::channel(2);
        let node_name = "test_node";
        // Create and run a mock Kubernetes API service and get a Kubernetes client
        let (client, _mock_service_task) = create_mock_kube_service(node_name).await;
        let node_status_patcher =
            NodeStatusPatcher::new(node_name, devices, update_node_status_sender, client);
        let status = node_status_patcher.get_node_status_patch().await;
        // Check that both resources listed under allocatable and only healthy devices are counted
        let allocatable = status.allocatable.unwrap();
        assert_eq!(allocatable.len(), 2);
        assert_eq!(
            allocatable.get(r1_name).unwrap(),
            &Quantity("2".to_string())
        );
        assert_eq!(
            allocatable.get(r2_name).unwrap(),
            &Quantity("2".to_string())
        );

        // Check that both resources listed under capacity and both healthy and unhealthy devices are counted
        let capacity = status.capacity.unwrap();
        assert_eq!(capacity.len(), 2);
        assert_eq!(capacity.get(r1_name).unwrap(), &Quantity("3".to_string()));
        assert_eq!(capacity.get(r2_name).unwrap(), &Quantity("2".to_string()));
    }
}
