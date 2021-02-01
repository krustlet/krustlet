use k8s_openapi::api::core::v1::HostPathVolumeSource;

use super::*;

pub(crate) async fn populate(hostpath: &HostPathVolumeSource) -> anyhow::Result<VolumeType> {
    // Check the the directory exists on the host
    tokio::fs::metadata(&hostpath.path).await?;
    Ok(VolumeType::HostPath)
}
