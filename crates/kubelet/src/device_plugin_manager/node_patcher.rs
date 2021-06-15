use super::{DeviceMap, HEALTHY};
use k8s_openapi::api::core::v1::Node;
use kube::api::{Api, PatchParams};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error};

/// NodePatcher updates the Node status with the latest device information.
#[derive(Clone)]
pub struct NodeStatusPatcher {
    node_name: String,
    devices: Arc<RwLock<DeviceMap>>,
    // Broadcast sender so clonable
    update_node_status_sender: broadcast::Sender<()>,
    client: kube::Client,
}

impl NodeStatusPatcher {
    pub fn new(
        node_name: &str,
        devices: Arc<RwLock<DeviceMap>>,
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

    async fn get_node_status_patch(&self) -> json_patch::Patch {
        let mut patches = Vec::new();
        let devices = self.devices.read().await;
        devices
            .iter()
            .for_each(|(resource_name, resource_devices)| {
                let adjusted_name = adjust_name(resource_name);
                let capacity_patch = serde_json::json!(
                    {
                        "op": "add",
                        "path": format!("/status/capacity/{}", adjusted_name),
                        "value": resource_devices.len().to_string()
                    }
                );
                let healthy_count: usize = resource_devices
                    .iter()
                    .filter(|(_, dev)| dev.health == HEALTHY)
                    .map(|(_, _)| 1)
                    .sum();
                let allocated_patch = serde_json::json!(
                    {
                        "op": "add",
                        "path": format!("/status/allocatable/{}", adjusted_name),
                        "value": healthy_count.to_string()
                    }
                );
                patches.push(capacity_patch);
                patches.push(allocated_patch);
            });
        let patches_value = serde_json::value::Value::Array(patches);
        json_patch::from_value(patches_value).unwrap()
    }

    async fn do_node_status_patch(&self, patch: json_patch::Patch) -> anyhow::Result<()> {
        debug!(
            "Patching {} node status with patch {:?}",
            self.node_name, patch
        );
        let node_client: Api<Node> = Api::all(self.client.clone());

        match node_client
            .patch_status(
                &self.node_name,
                &PatchParams::default(),
                &kube::api::Patch::Json::<()>(patch),
            )
            .await
        {
            Err(e) => Err(anyhow::anyhow!("Unable to patch node status: {}", e)),
            Ok(s) => {
                debug!("Node status patch returned {:?}", s);
                Ok(())
            }
        }
    }

    pub async fn listen_and_patch(self) -> anyhow::Result<()> {
        let mut receiver = self.update_node_status_sender.subscribe();
        loop {
            match receiver.recv().await {
                Err(_e) => {
                    error!("Channel closed by senders");
                    // TODO: bubble up error
                }
                Ok(_) => {
                    debug!("Received notification that Node status should be patched");
                    // Grab status values
                    let status_patch = self.get_node_status_patch().await;
                    // Do patch
                    self.do_node_status_patch(status_patch).await?;
                }
            }
        }
    }
}

fn adjust_name(name: &str) -> String {
    name.replace("/", "~1")
}

#[cfg(test)]
mod tests {
    use super::super::test_utils::{create_mock_healthy_devices, create_mock_kube_service};
    use super::super::UNHEALTHY;
    use super::*;

    #[test]
    fn test_adjust_name() {
        assert_eq!(adjust_name("example.com/r1"), "example.com~1r1");
    }

    #[tokio::test]
    async fn test_do_node_status_patch() {
        let devices = create_mock_healthy_devices("r1", "r2");
        devices
            .write()
            .await
            .get_mut("r1")
            .unwrap()
            .get_mut("r1-id1")
            .unwrap()
            .health = UNHEALTHY.to_string();
        let patch_value = serde_json::json!([
            {
                "op": "add",
                "path": format!("/status/capacity/example.com~1foo"),
                "value": "2"
            }
        ]);
        let patch = json_patch::from_value(patch_value).unwrap();
        let (update_node_status_sender, _rx) = broadcast::channel(2);

        // Create and run a mock Kubernetes API service and get a Kubernetes client
        let (client, _mock_service_task) = create_mock_kube_service("test_node").await;
        let node_name = "test_node";
        let node_status_patcher =
            NodeStatusPatcher::new(node_name, devices, update_node_status_sender, client);
        node_status_patcher
            .do_node_status_patch(patch)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_get_node_status_patch() {
        let r1_name = "example.com/r1";
        let r2_name = "something.net/r2";
        let devices = create_mock_healthy_devices(r1_name, r2_name);
        devices
            .write()
            .await
            .get_mut(r1_name)
            .unwrap()
            .get_mut(&format!("{}-id1", r1_name))
            .unwrap()
            .health = UNHEALTHY.to_string();
        let (update_node_status_sender, _rx) = broadcast::channel(2);
        let node_name = "test_node";
        // Create and run a mock Kubernetes API service and get a Kubernetes client
        let (client, _mock_service_task) = create_mock_kube_service(node_name).await;
        let node_status_patcher =
            NodeStatusPatcher::new(node_name, devices, update_node_status_sender, client);
        let patch = node_status_patcher.get_node_status_patch().await;
        let expected_patch_value = serde_json::json!([
            {
                "op": "add",
                "path": format!("/status/capacity/example.com~1r1"),
                "value": "3"
            },
            {
                "op": "add",
                "path": format!("/status/allocatable/example.com~1r1"),
                "value": "2"
            },
            {
                "op": "add",
                "path": format!("/status/capacity/something.net~1r2"),
                "value": "2"
            },
            {
                "op": "add",
                "path": format!("/status/allocatable/something.net~1r2"),
                "value": "2"
            }
        ]);
        let expected_patch = json_patch::from_value(expected_patch_value).unwrap();
        // Check that both resources listed under allocatable and only healthy devices are counted
        // Check that both resources listed under capacity and both healthy and unhealthy devices are counted
        assert_eq!(patch, expected_patch);
    }
}
