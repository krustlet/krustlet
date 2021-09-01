pub mod setup;
#[cfg(target_os = "linux")]
use crate::grpc_sock;
use std::collections::HashSet;
use std::sync::Arc;

use k8s_csi::v1_3_0::*;
use k8s_csi::v1_3_0::{
    controller_server::Controller,
    controller_service_capability::rpc::Type as RpcType,
    controller_service_capability::{Rpc, Type},
    identity_server::Identity,
    list_volumes_response::{Entry, VolumeStatus},
    node_server::Node,
    plugin_capability::service::Type as PluginRpcType,
    plugin_capability::{Service, Type as PluginType},
    validate_volume_capabilities_response::Confirmed,
};
use tokio::sync::RwLock;

pub const DRIVER_NAME: &str = "mock.csi.krustlet.dev";

#[derive(Clone)]
pub struct MockCsiPlugin {
    volumes: Arc<RwLock<HashSet<String>>>,
    node_name: String,
    pub node_publish_called: Arc<RwLock<bool>>,
    pub node_unpublish_called: Arc<RwLock<bool>>,
}

impl MockCsiPlugin {
    pub fn new(node_name: &str) -> Self {
        MockCsiPlugin {
            volumes: Arc::new(RwLock::new(HashSet::new())),
            node_name: node_name.to_owned(),
            node_publish_called: Default::default(),
            node_unpublish_called: Default::default(),
        }
    }
}

#[async_trait::async_trait]
impl Controller for MockCsiPlugin {
    /// Does nothing except keep track of the volume name. The module should
    /// just use the empty directory created by krustlet
    async fn create_volume(
        &self,
        request: tonic::Request<CreateVolumeRequest>,
    ) -> Result<tonic::Response<CreateVolumeResponse>, tonic::Status> {
        let req = request.into_inner();
        let mut vols = self.volumes.write().await;
        vols.insert(req.name.clone());
        Ok(tonic::Response::new(CreateVolumeResponse {
            volume: Some(Volume {
                volume_id: req.name,
                capacity_bytes: 0,
                ..Default::default()
            }),
        }))
    }

    /// Removes the volume name from tracking
    async fn delete_volume(
        &self,
        request: tonic::Request<DeleteVolumeRequest>,
    ) -> Result<tonic::Response<DeleteVolumeResponse>, tonic::Status> {
        let req = request.into_inner();
        let mut vols = self.volumes.write().await;
        vols.remove(&req.volume_id);
        Ok(tonic::Response::new(DeleteVolumeResponse {}))
    }

    async fn controller_publish_volume(
        &self,
        _request: tonic::Request<ControllerPublishVolumeRequest>,
    ) -> Result<tonic::Response<ControllerPublishVolumeResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "controller publish volume not implemented",
        ))
    }

    async fn controller_unpublish_volume(
        &self,
        _request: tonic::Request<ControllerUnpublishVolumeRequest>,
    ) -> Result<tonic::Response<ControllerUnpublishVolumeResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "controller unpublish volume not implemented",
        ))
    }

    async fn validate_volume_capabilities(
        &self,
        request: tonic::Request<ValidateVolumeCapabilitiesRequest>,
    ) -> Result<tonic::Response<ValidateVolumeCapabilitiesResponse>, tonic::Status> {
        let req = request.into_inner();
        // Super basic validation to emulate things somewhat
        let vols = self.volumes.read().await;
        if !vols.contains(&req.volume_id) {
            return Err(tonic::Status::not_found("volume not found"));
        }

        for cap in req.volume_capabilities.iter() {
            if cap.access_type.is_none() {
                return Err(tonic::Status::invalid_argument(
                    "An access type must be specified",
                ));
            }
        }
        Ok(tonic::Response::new(ValidateVolumeCapabilitiesResponse {
            confirmed: Some(Confirmed {
                volume_context: req.volume_context,
                volume_capabilities: req.volume_capabilities,
                parameters: req.parameters,
            }),
            message: String::new(),
        }))
    }

    async fn list_volumes(
        &self,
        _request: tonic::Request<ListVolumesRequest>,
    ) -> Result<tonic::Response<ListVolumesResponse>, tonic::Status> {
        let vols = self.volumes.read().await;
        // We are ignoring pagination here
        Ok(tonic::Response::new(ListVolumesResponse {
            next_token: String::new(),
            entries: vols
                .iter()
                .cloned()
                .map(|volume_id| Entry {
                    volume: Some(Volume {
                        volume_id,
                        capacity_bytes: 0,
                        ..Default::default()
                    }),
                    status: Some(VolumeStatus {
                        published_node_ids: vec![self.node_name.clone()],
                        volume_condition: Some(VolumeCondition {
                            abnormal: false,
                            message: String::from("Volume is a-ok"),
                        }),
                    }),
                })
                .collect(),
        }))
    }

    async fn get_capacity(
        &self,
        _request: tonic::Request<GetCapacityRequest>,
    ) -> Result<tonic::Response<GetCapacityResponse>, tonic::Status> {
        Ok(tonic::Response::new(GetCapacityResponse {
            available_capacity: 104857600, // 100 GB
        }))
    }

    async fn controller_get_capabilities(
        &self,
        _request: tonic::Request<ControllerGetCapabilitiesRequest>,
    ) -> Result<tonic::Response<ControllerGetCapabilitiesResponse>, tonic::Status> {
        Ok(tonic::Response::new(ControllerGetCapabilitiesResponse {
            capabilities: vec![
                ControllerServiceCapability {
                    r#type: Some(Type::Rpc(Rpc {
                        r#type: RpcType::CreateDeleteVolume as i32,
                    })),
                },
                ControllerServiceCapability {
                    r#type: Some(Type::Rpc(Rpc {
                        r#type: RpcType::GetVolume as i32,
                    })),
                },
                ControllerServiceCapability {
                    r#type: Some(Type::Rpc(Rpc {
                        r#type: RpcType::GetCapacity as i32,
                    })),
                },
                ControllerServiceCapability {
                    r#type: Some(Type::Rpc(Rpc {
                        r#type: RpcType::ListVolumes as i32,
                    })),
                },
                ControllerServiceCapability {
                    r#type: Some(Type::Rpc(Rpc {
                        r#type: RpcType::VolumeCondition as i32,
                    })),
                },
            ],
        }))
    }

    async fn create_snapshot(
        &self,
        _request: tonic::Request<CreateSnapshotRequest>,
    ) -> Result<tonic::Response<CreateSnapshotResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("snapshots not implemented"))
    }

    async fn delete_snapshot(
        &self,
        _request: tonic::Request<DeleteSnapshotRequest>,
    ) -> Result<tonic::Response<DeleteSnapshotResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("snapshots not implemented"))
    }

    async fn list_snapshots(
        &self,
        _request: tonic::Request<ListSnapshotsRequest>,
    ) -> Result<tonic::Response<ListSnapshotsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("snapshots not implemented"))
    }

    async fn controller_expand_volume(
        &self,
        _request: tonic::Request<ControllerExpandVolumeRequest>,
    ) -> Result<tonic::Response<ControllerExpandVolumeResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "expand volume not implemented",
        ))
    }

    async fn controller_get_volume(
        &self,
        request: tonic::Request<ControllerGetVolumeRequest>,
    ) -> Result<tonic::Response<ControllerGetVolumeResponse>, tonic::Status> {
        let req = request.into_inner();
        self.volumes
            .read()
            .await
            .contains(&req.volume_id)
            .then(|| {
                tonic::Response::new(ControllerGetVolumeResponse {
                    volume: Some(Volume {
                        volume_id: req.volume_id,
                        capacity_bytes: 0,
                        ..Default::default()
                    }),
                    status: Some(controller_get_volume_response::VolumeStatus {
                        published_node_ids: vec![self.node_name.clone()],
                        volume_condition: Some(VolumeCondition {
                            abnormal: false,
                            message: String::from("Volume is a-ok"),
                        }),
                    }),
                })
            })
            .ok_or_else(|| tonic::Status::not_found("Volume not found"))
    }
}

#[async_trait::async_trait]
impl Node for MockCsiPlugin {
    /// Our "publish" volume does nothing except check that the volume exists
    /// and mark that the function was called
    async fn node_publish_volume(
        &self,
        request: tonic::Request<NodePublishVolumeRequest>,
    ) -> Result<tonic::Response<NodePublishVolumeResponse>, tonic::Status> {
        let req = request.into_inner();
        if !self.volumes.read().await.contains(&req.volume_id) {
            return Err(tonic::Status::not_found("volume not found"));
        }
        let mut called = self.node_publish_called.write().await;
        *called = true;
        Ok(tonic::Response::new(NodePublishVolumeResponse {}))
    }

    async fn node_stage_volume(
        &self,
        _request: tonic::Request<NodeStageVolumeRequest>,
    ) -> Result<tonic::Response<NodeStageVolumeResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("stage/unstage not supported"))
    }

    async fn node_unstage_volume(
        &self,
        _request: tonic::Request<NodeUnstageVolumeRequest>,
    ) -> Result<tonic::Response<NodeUnstageVolumeResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("stage/unstage not supported"))
    }

    /// Our "unpublish" volume does nothing except check that the volume exists
    /// and mark that the function was called
    async fn node_unpublish_volume(
        &self,
        request: tonic::Request<NodeUnpublishVolumeRequest>,
    ) -> Result<tonic::Response<NodeUnpublishVolumeResponse>, tonic::Status> {
        let req = request.into_inner();
        if !self.volumes.read().await.contains(&req.volume_id) {
            return Err(tonic::Status::not_found("volume not found"));
        }
        let mut called = self.node_unpublish_called.write().await;
        *called = true;
        Ok(tonic::Response::new(NodeUnpublishVolumeResponse {}))
    }

    async fn node_get_volume_stats(
        &self,
        _request: tonic::Request<NodeGetVolumeStatsRequest>,
    ) -> Result<tonic::Response<NodeGetVolumeStatsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("volume stats not supported"))
    }

    async fn node_expand_volume(
        &self,
        _request: tonic::Request<NodeExpandVolumeRequest>,
    ) -> Result<tonic::Response<NodeExpandVolumeResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("expand volume not supported"))
    }

    async fn node_get_capabilities(
        &self,
        _request: tonic::Request<NodeGetCapabilitiesRequest>,
    ) -> Result<tonic::Response<NodeGetCapabilitiesResponse>, tonic::Status> {
        Ok(tonic::Response::new(NodeGetCapabilitiesResponse {
            // We don't support any of the extras here
            capabilities: Vec::with_capacity(0),
        }))
    }

    async fn node_get_info(
        &self,
        _request: tonic::Request<NodeGetInfoRequest>,
    ) -> Result<tonic::Response<NodeGetInfoResponse>, tonic::Status> {
        let mut segments = std::collections::BTreeMap::new();
        segments.insert(
            "topology.hostpath.csi/node".to_owned(),
            self.node_name.clone(),
        );
        Ok(tonic::Response::new(NodeGetInfoResponse {
            node_id: self.node_name.clone(),
            max_volumes_per_node: 20,
            accessible_topology: Some(Topology { segments }),
        }))
    }
}

#[async_trait::async_trait]
impl Identity for MockCsiPlugin {
    async fn get_plugin_info(
        &self,
        _request: tonic::Request<GetPluginInfoRequest>,
    ) -> Result<tonic::Response<GetPluginInfoResponse>, tonic::Status> {
        Ok(tonic::Response::new(GetPluginInfoResponse {
            name: DRIVER_NAME.to_owned(),
            vendor_version: String::from("v1.0.0"),
            ..Default::default()
        }))
    }

    async fn get_plugin_capabilities(
        &self,
        _request: tonic::Request<GetPluginCapabilitiesRequest>,
    ) -> Result<tonic::Response<GetPluginCapabilitiesResponse>, tonic::Status> {
        Ok(tonic::Response::new(GetPluginCapabilitiesResponse {
            capabilities: vec![PluginCapability {
                r#type: Some(PluginType::Service(Service {
                    r#type: PluginRpcType::ControllerService as i32,
                })),
            }],
        }))
    }

    async fn probe(
        &self,
        _request: tonic::Request<ProbeRequest>,
    ) -> Result<tonic::Response<ProbeResponse>, tonic::Status> {
        Ok(tonic::Response::new(ProbeResponse { ready: Some(true) }))
    }
}
