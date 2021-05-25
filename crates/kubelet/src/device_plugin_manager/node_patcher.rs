
use super::{DeviceMap, HEALTHY, UNHEALTHY};
use kube::api::{Api, PatchParams};
use k8s_openapi::api::core::v1::{Node, NodeStatus};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
#[cfg(test)]
use mockall::{automock, predicate::*};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[cfg_attr(test, automock)]
#[async_trait::async_trait]
pub trait NodeStatusPatcher: Send + Sync + 'static {
    async fn get_node_status_patch(
        &self,
    ) -> NodeStatus;

    async fn do_node_status_patch(
        &self,
        status: NodeStatus,
        node_name: &str,
        client: &kube::Client,
    ) -> anyhow::Result<()>;
}

/// NodePatcher updates the Node status with the latest device information.
#[derive(Clone)]
pub struct NodeStatusPatcherImpl {
    pub devices: Arc<Mutex<DeviceMap>>,
}

#[async_trait::async_trait]
impl NodeStatusPatcher for NodeStatusPatcherImpl {
    async fn get_node_status_patch(
        &self,
        // devices: Arc<Mutex<DeviceMap>>,
    ) -> NodeStatus {
        let devices = self.devices.lock().unwrap();
        let capacity: BTreeMap<String, Quantity> = devices.iter().map(|(resource_name, resource_devices)| (resource_name.clone(), Quantity(resource_devices.len().to_string()))).collect();
        let allocatable: BTreeMap<String, Quantity> = devices.iter().map(|(resource_name, resource_devices)| {
            let healthy_count: usize = resource_devices.iter().filter(|(_, dev)| dev.health == HEALTHY).map(|(_, _)| 1).sum();
            (resource_name.clone(), Quantity(healthy_count.to_string()))
        }).collect();
        // let allocated: BTreeMap<String, Quantity> = self.allocated_device_ids.lock().unwrap().iter().map(|(resource_name, ids)| (resource_name.clone(), Quantity(ids.len().to_string()))).collect();
        // Update Node status
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
}

pub async fn listen_and_patch(
    update_node_status_receiver: mpsc::Receiver<()>,
    node_name: String,
    client: kube::Client,
    node_status_patcher: impl NodeStatusPatcher,
) -> anyhow::Result<()> {
    let mut receiver = update_node_status_receiver;
    println!("entered listen_and_patch");
    loop {
        println!("listen_and_patch loop");
        match receiver.recv().await {
            None => {
                error!("Channel closed by senders");
                // TODO: bubble up error
            },
            Some(_) => {
                // Grab status values
                let status_patch = node_status_patcher.get_node_status_patch().await;
                // Do patch 
                node_status_patcher.do_node_status_patch(status_patch, &node_name, &client).await?;
            }
        }
    }
    // TODO add channel for termination?
    Ok(())
}




#[cfg(test)]
mod node_patcher_tests {
    use super::super::{EndpointDevicesMap, UNHEALTHY};
    use super::*;
    use crate::device_plugin_api::v1beta1::Device;

    // #[tokio::test]
    // async fn test_listen_and_patch() {
    //     println!("running test_get_node_status_patch");
    //     let r1d1 = Device{id: "r1-id1".to_string(), health: HEALTHY.to_string(), topology: None};
    //     let r1d2 =  Device{id: "r1-id2".to_string(), health: HEALTHY.to_string(), topology: None};
    //     let r1_devices: EndpointDevicesMap = [
    //         ("r1-id1".to_string(), Device{id: "r1-id1".to_string(), health: HEALTHY.to_string(), topology: None}), 
    //         ("r1-id2".to_string(), Device{id: "r1-id2".to_string(), health: HEALTHY.to_string(), topology: None}),
    //         ("r1-id3".to_string(), Device{id: "r1-id3".to_string(), health: UNHEALTHY.to_string(), topology: None})
    //         ].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    //     let r2_devices: EndpointDevicesMap = [
    //         ("r2-id1".to_string(), Device{id: "r2-id1".to_string(), health: HEALTHY.to_string(), topology: None}), 
    //         ("r2-id2".to_string(), Device{id: "r2-id2".to_string(), health: HEALTHY.to_string(), topology: None})
    //         ].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    //     let r1_name = "r1".to_string();
    //     let r2_name = "r2".to_string();
    //     let device_map: DeviceMap = [(r1_name.clone(), r1_devices), (r2_name.clone(), r2_devices)].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    //     let devices = Arc::new(Mutex::new(device_map));
    //     let (update_node_status_sender, update_node_status_receiver) = mpsc::channel(2);

    //     let node_status_patcher = NodeStatusPatcher { devices, update_node_status_receiver};
    //     update_node_status_sender.send(()).await.unwrap();
    //     node_status_patcher.listen_and_patch().await.unwrap();
    //     // ...
    // }

    #[tokio::test]
    async fn test_get_node_status_patch() {
        println!("running test_get_node_status_patch");
        let r1_devices: EndpointDevicesMap = [
            ("r1-id1".to_string(), Device{id: "r1-id1".to_string(), health: HEALTHY.to_string(), topology: None}), 
            ("r1-id2".to_string(), Device{id: "r1-id2".to_string(), health: HEALTHY.to_string(), topology: None}),
            ("r1-id3".to_string(), Device{id: "r1-id3".to_string(), health: UNHEALTHY.to_string(), topology: None})
            ].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let r2_devices: EndpointDevicesMap = [
            ("r2-id1".to_string(), Device{id: "r2-id1".to_string(), health: HEALTHY.to_string(), topology: None}), 
            ("r2-id2".to_string(), Device{id: "r2-id2".to_string(), health: HEALTHY.to_string(), topology: None})
            ].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let r1_name = "r1".to_string();
        let r2_name = "r2".to_string();
        let device_map: DeviceMap = [(r1_name.clone(), r1_devices), (r2_name.clone(), r2_devices)].iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let devices = Arc::new(Mutex::new(device_map));
        let node_status_patcher = NodeStatusPatcherImpl { devices};
        let status = node_status_patcher.get_node_status_patch().await;
        // Check that both resources listed under allocatable and only healthy devices are counted
        let allocatable = status.allocatable.unwrap();
        assert_eq!(allocatable.len(), 2);
        assert_eq!(allocatable.get(&r1_name).unwrap(), &Quantity("2".to_string()));
        assert_eq!(allocatable.get(&r2_name).unwrap(), &Quantity("2".to_string()));

        // Check that both resources listed under capacity and both healthy and unhealthy devices are counted
        let capacity = status.capacity.unwrap();
        assert_eq!(capacity.len(), 2);
        assert_eq!(capacity.get(&r1_name).unwrap(), &Quantity("3".to_string()));
        assert_eq!(capacity.get(&r2_name).unwrap(), &Quantity("2".to_string()));

    }
}