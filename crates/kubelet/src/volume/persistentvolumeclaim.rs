use std::path::Path;
use std::str::FromStr;

use k8s_csi::v1_3_0::node_client::NodeClient;
use k8s_csi::v1_3_0::node_service_capability::{rpc, Rpc, Type as CapabilityType};
use k8s_csi::v1_3_0::volume_capability::access_mode::Mode as CSIMode;
use k8s_csi::v1_3_0::volume_capability::{
    AccessMode as CSIAccessMode, AccessType as CSIAccessType, MountVolume as CSIMountVolume,
};
use k8s_csi::v1_3_0::{
    NodeGetCapabilitiesRequest, NodePublishVolumeRequest, NodeStageVolumeRequest,
    NodeUnpublishVolumeRequest, VolumeCapability,
};

use k8s_openapi::api::core::v1::{
    CSIPersistentVolumeSource, PersistentVolume, PersistentVolumeClaimSpec,
    PersistentVolumeClaimVolumeSource,
};
use k8s_openapi::api::storage::v1::StorageClass;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;

use tempdir::TempDir;
use thiserror::Error;

use crate::grpc_sock;
use crate::plugin_watcher::PluginRegistry;

use super::*;

/// VolumeError describes the possible error states when mounting persistent volume claims.
#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
enum VolumeError {
    #[error("bad volume mode")]
    BadVolumeMode,
    #[error("bad reclaim policy")]
    BadReclaimPolicy,
    #[error("bad access mode")]
    BadAccessMode,
}

// Kubernetes supports two volumeModes of PersistentVolumes: Filesystem and
// Block.
//
// volumeMode is an optional API parameter. Filesystem is the default mode used
// when volumeMode parameter is omitted.
//
// A volume with volumeMode: Filesystem is mounted into Pods into a directory.
// If the volume is backed by a block device and the device is empty,
// Kuberneretes creates a filesystem on the device before mounting it for the
// first time.
//
// You can set the value of volumeMode to Block to use a volume as a raw block
// device. Such volume is presented into a Pod as a block device, without any
// filesystem on it. This mode is useful to provide a Pod the fastest possible
// way to access a volume, without any filesystem layer between the Pod and the
// volume. On the other hand, the application running in the Pod must know how
// to handle a raw block device. See Raw Block Volume Support for an example on
// how to use a volume with volumeMode: Block in a Pod.
//
// https://kubernetes.io/docs/concepts/storage/persistent-volumes/#volume-mode
#[derive(Debug)]
enum VolumeMode {
    Block,
    Filesystem,
}

impl FromStr for VolumeMode {
    type Err = VolumeError;

    // defines what type of volume is required by the claim. The "filesystem"
    // mode is implied when not included in the claim spec.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Block" => Ok(VolumeMode::Block),
            "Filesystem" => Ok(VolumeMode::Filesystem),
            "" => Ok(VolumeMode::Filesystem),
            _ => Err(VolumeError::BadVolumeMode),
        }
    }
}

#[derive(Debug)]
enum ReclaimPolicy {
    Delete,
    Recycle,
    Retain,
}

impl FromStr for ReclaimPolicy {
    type Err = VolumeError;

    // defines what type of reclaim policy is requested. The "delete" mode is
    // implied when not included in the storage class or persistent volume.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Delete" => Ok(ReclaimPolicy::Delete),
            "Recycle" => Ok(ReclaimPolicy::Recycle),
            "Retain" => Ok(ReclaimPolicy::Retain),
            "" => Ok(ReclaimPolicy::Delete),
            _ => Err(VolumeError::BadReclaimPolicy),
        }
    }
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum AccessMode {
    ReadOnlyMany,
    ReadWriteMany,
    ReadWriteOnce,
}

impl FromStr for AccessMode {
    type Err = VolumeError;

    // defines what type of access mode is requested. An error is returned when
    // an invalid access mode is provided.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ReadOnlyMany" => Ok(AccessMode::ReadOnlyMany),
            "ReadWriteMany" => Ok(AccessMode::ReadWriteMany),
            "ReadWriteOnce" => Ok(AccessMode::ReadWriteOnce),
            _ => Err(VolumeError::BadAccessMode),
        }
    }
}

// Validates a PersistentVolumeClaimSpec.
// https://github.com/kubernetes/kubernetes/blob/c970a46bc1bcc100bbbfabd5c12bd4c5d87f8aea/pkg/apis/core/validation/validation.go#L1965
pub(crate) fn validate(spec: &PersistentVolumeClaimSpec) -> anyhow::Result<()> {
    match &spec.access_modes {
        Some(a) => {
            if a.is_empty() {
                return Err(anyhow::anyhow!("at least 1 access mode is required"));
            } else {
                for access_mode in a {
                    // validate access modes are correct
                    AccessMode::from_str(access_mode)?;
                }
            }
        }
        None => {
            return Err(anyhow::anyhow!("at least 1 access mode is required"));
        }
    }

    if let Some(selector) = &spec.selector {
        validate_label_selector(selector)?;
    }

    match &spec.storage_class_name {
        // no storage class name specified or set to the empty string
        // means the user requested the "default" storage class. there
        // is no "default" storage class for Krustlet at this time, so
        // we treat this as an error case.
        //
        // TODO: implement a "default" storage class
        None => {
            return Err(anyhow::anyhow!(
                "PersistentVolumeClaim must specify a storage class"
            ));
        }
        Some(s) => {
            if s.is_empty() {
                return Err(anyhow::anyhow!(
                    "PersistentVolumeClaim must specify a storage class"
                ));
            }
            s
        }
    };

    // TODO: validate volume mode

    // TODO: validate data source

    Ok(())
}

// TODO: remove this allow once failing validations are added.
#[allow(clippy::unnecessary_wraps)]
fn validate_label_selector(_selector: &LabelSelector) -> anyhow::Result<()> {
    // TODO: validate label selectors
    Ok(())
}

pub(crate) async fn populate(
    pvc_source: &PersistentVolumeClaimVolumeSource,
    client: &kube::Client,
    namespace: &str,
    pr: Option<Arc<PluginRegistry>>,
    path: &Path,
) -> anyhow::Result<VolumeType> {
    if pr.is_none() {
        return Err(anyhow::anyhow!(format!(
            "failed to mount volume {}: CSI driver support not implemented",
            &pvc_source.claim_name
        )));
    }
    let plugin_registry = pr.unwrap();

    let spec = get_pvc_spec(pvc_source, client, namespace).await?;
    let mut csi_client = get_csi_client(client, &spec, plugin_registry).await?;
    let csi = get_csi(client, pvc_source, &spec).await?;
    let stage_unstage_volume = supports_stage_unstage(&mut csi_client).await?;

    // we keep this around even if the driver does not support STAGE_UNSTAGE_VOLUME. unmount() still needs it.
    tokio::fs::create_dir_all(path).await?;
    // TODO(bacongobbler): implement node_unstage_volume(). We'll need to
    // persist the staging_path somewhere so we can recall that information
    // during unpopulate()
    let staging_path = TempDir::new(&csi.volume_handle)?;
    if stage_unstage_volume {
        stage_volume(&mut csi_client, &csi, staging_path.path()).await?;
    }
    publish_volume(
        &mut csi_client,
        &csi,
        staging_path.path(),
        stage_unstage_volume,
        path,
    )
    .await?;

    Ok(VolumeType::PersistentVolumeClaim)
}

pub(crate) async fn unpopulate(
    pvc_source: &PersistentVolumeClaimVolumeSource,
    client: &kube::Client,
    namespace: &str,
    pr: Option<Arc<PluginRegistry>>,
    path: &Path,
) -> anyhow::Result<()> {
    if pr.is_none() {
        return Err(anyhow::anyhow!(format!(
            "failed to unmount volume {}: CSI driver support not implemented",
            &pvc_source.claim_name
        )));
    }
    let plugin_registry = pr.unwrap();

    let spec = get_pvc_spec(pvc_source, client, namespace).await?;
    let mut csi_client = get_csi_client(client, &spec, plugin_registry).await?;
    let csi = get_csi(client, pvc_source, &spec).await?;

    // https://github.com/kubernetes/kubernetes/blob/6d5cb36d36f34cb4f5735b6adcd5ea8ebb4440ba/pkg/volume/csi/csi_mounter.go#L390
    unpublish_volume(&mut csi_client, &csi, path).await?;
    std::fs::remove_dir_all(path)?;

    Ok(())
}

// checks if the plugin supports the node_stage/unstage_volume API. Assume false if not specified.
async fn supports_stage_unstage(
    csi_client: &mut NodeClient<tonic::transport::Channel>,
) -> anyhow::Result<bool> {
    let mut stage_unstage_volume = false;
    let response = csi_client
        .node_get_capabilities(NodeGetCapabilitiesRequest {})
        .await?;
    for capability in &response.get_ref().capabilities {
        if let Some(typ) = &capability.r#type {
            let _typ_stage_unstage_volume = rpc::Type::StageUnstageVolume as i32;
            match typ {
                CapabilityType::Rpc(Rpc {
                    r#type: _typ_stage_unstage_volume,
                }) => {
                    stage_unstage_volume = true;
                }
            }
        }
    }
    Ok(stage_unstage_volume)
}

async fn stage_volume(
    csi_client: &mut NodeClient<tonic::transport::Channel>,
    csi: &CSIPersistentVolumeSource,
    staging_path: &Path,
) -> anyhow::Result<()> {
    // TODO: grab the publish_context and volume_context using the volume attachments API
    csi_client
        .node_stage_volume(NodeStageVolumeRequest {
            volume_id: csi.volume_handle.clone(),
            staging_target_path: staging_path.to_string_lossy().to_string(),
            volume_capability: Some(VolumeCapability {
                // TODO: determine the correct access mode and mount flags from the volume
                // https://github.com/kubernetes/kubernetes/blob/734889ed822d1a60c6dd61ccd8f1ed0e8ab31ea5/pkg/volume/csi/csi_attacher.go#L325-L333
                access_mode: Some(CSIAccessMode {
                    mode: CSIMode::SingleNodeWriter as i32,
                }),
                access_type: Some(CSIAccessType::Mount(CSIMountVolume {
                    fs_type: csi.fs_type.as_ref().map_or("".to_owned(), |s| s.clone()),
                    mount_flags: Default::default(),
                })),
            }),
            secrets: Default::default(),
            publish_context: Default::default(),
            volume_context: Default::default(),
        })
        .await?;
    Ok(())
}

async fn publish_volume(
    csi_client: &mut NodeClient<tonic::transport::Channel>,
    csi: &CSIPersistentVolumeSource,
    staging_path: &Path,
    stage_unstage_volume: bool,
    path: &Path,
) -> anyhow::Result<()> {
    let mut req = NodePublishVolumeRequest {
        volume_id: csi.volume_handle.clone(),
        target_path: path.to_string_lossy().to_string(),
        staging_target_path: "".to_owned(),
        volume_capability: Some(VolumeCapability {
            // TODO: determine the correct access mode and mount flags from the volume
            // https://github.com/kubernetes/kubernetes/blob/734889ed822d1a60c6dd61ccd8f1ed0e8ab31ea5/pkg/volume/csi/csi_attacher.go#L325-L333
            access_mode: Some(CSIAccessMode {
                mode: CSIMode::SingleNodeWriter as i32,
            }),
            access_type: Some(CSIAccessType::Mount(CSIMountVolume {
                fs_type: csi.fs_type.clone().unwrap_or_default(),
                mount_flags: Default::default(),
            })),
        }),
        // hardcode to read/write for now
        // TODO: determine the correct access mode and mount flags from the volume
        // https://github.com/kubernetes/kubernetes/blob/734889ed822d1a60c6dd61ccd8f1ed0e8ab31ea5/pkg/volume/csi/csi_attacher.go#L325-L333
        readonly: false,
        secrets: Default::default(),
        publish_context: Default::default(),
        volume_context: Default::default(),
    };
    if stage_unstage_volume {
        req.staging_target_path = staging_path.to_string_lossy().to_string();
    }
    csi_client.node_publish_volume(req).await?;
    Ok(())
}

async fn unpublish_volume(
    csi_client: &mut NodeClient<tonic::transport::Channel>,
    csi: &CSIPersistentVolumeSource,
    path: &Path,
) -> anyhow::Result<()> {
    let req = NodeUnpublishVolumeRequest {
        volume_id: csi.volume_handle.clone(),
        target_path: path.to_string_lossy().to_string(),
    };
    csi_client.node_unpublish_volume(req).await?;
    Ok(())
}

async fn get_csi_client(
    client: &kube::Client,
    spec: &PersistentVolumeClaimSpec,
    plugin_registry: Arc<PluginRegistry>,
) -> anyhow::Result<NodeClient<tonic::transport::Channel>> {
    let storage_class_client: Api<StorageClass> = Api::all(client.clone());
    // NOTE(bacongobbler): should be safe to unwrap() here after calling
    // validate(). Storage class names are required as we do not define
    // a default storage class.
    //
    // If we do implement a default storage class, then we'll have to
    // revisit this assumption (omission of this field implies the
    // default).
    //
    // https://kubernetes.io/docs/concepts/storage/persistent-volumes/#class-1
    let storage_class = storage_class_client
        .get(spec.storage_class_name.as_ref().unwrap())
        .await?;
    let endpoint = plugin_registry
        .get_endpoint(&storage_class.provisioner)
        .await
        .ok_or_else(|| anyhow::anyhow!("could not get CSI plugin endpoint"))?;
    let chan = grpc_sock::client::socket_channel(endpoint).await?;
    Ok(NodeClient::new(chan))
}

async fn get_csi(
    client: &kube::Client,
    pvc_source: &PersistentVolumeClaimVolumeSource,
    spec: &PersistentVolumeClaimSpec,
) -> anyhow::Result<CSIPersistentVolumeSource> {
    let volume_name = spec.volume_name.as_ref().ok_or(anyhow::anyhow!(format!(
        "volume name for PVC {} must exist",
        pvc_source.claim_name
    )))?;
    // TODO(bacongobbler): When a PVC specifies a selector in addition to
    // requesting a StorageClass, the requirements are ANDed together: only
    // a PV of the requested class and with the requested labels may be
    // bound to the PVC.
    // https://kubernetes.io/docs/concepts/storage/persistent-volumes/#class-1
    let pv_client: Api<PersistentVolume> = Api::all(client.clone());
    let pv = pv_client.get(&volume_name).await?;

    // https://github.com/kubernetes/kubernetes/blob/734889ed822d1a60c6dd61ccd8f1ed0e8ab31ea5/pkg/volume/csi/csi_attacher.go#L295-L298
    let csi = pv
        .spec
        .ok_or_else(|| anyhow::anyhow!("no PersistentVolume spec defined"))?
        .csi
        .ok_or_else(|| anyhow::anyhow!("no CSI spec defined"))?;
    Ok(csi)
}

async fn get_pvc_spec(
    pvc_source: &PersistentVolumeClaimVolumeSource,
    client: &kube::Client,
    namespace: &str,
) -> anyhow::Result<PersistentVolumeClaimSpec> {
    let pvc_client: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), namespace);

    let pvc = pvc_client.get(&pvc_source.claim_name).await?;
    let spec = match pvc.spec {
        Some(s) => s,
        None => {
            return Err(anyhow::anyhow!("PersistentVolumeClaim must specify a spec"));
        }
    };
    validate(&spec)?;
    Ok(spec)
}
