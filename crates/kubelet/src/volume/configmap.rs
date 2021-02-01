use std::path::PathBuf;

use k8s_openapi::api::core::v1::{ConfigMap, KeyToPath};

use super::*;

pub(crate) async fn populate(
    config_map: ConfigMap,
    path: &PathBuf,
    items: &Option<Vec<KeyToPath>>,
) -> anyhow::Result<VolumeType> {
    tokio::fs::create_dir_all(path).await?;
    let binary_data = config_map.binary_data.unwrap_or_default();
    let binary_data = binary_data.into_iter().map(|(key, data)| async move {
        match mount_setting_for(&key, items) {
            ItemMount::MountAt(mount_path) => {
                let file_path = path.join(mount_path);
                tokio::fs::write(file_path, &data.0).await
            }
            ItemMount::DoNotMount => Ok(()),
        }
    });
    let binary_data = futures::future::join_all(binary_data);
    let data = config_map.data.unwrap_or_default();
    let data = data.into_iter().map(|(key, data)| async move {
        match mount_setting_for(&key, items) {
            ItemMount::MountAt(mount_path) => {
                let file_path = path.join(mount_path);
                tokio::fs::write(file_path, data).await
            }
            ItemMount::DoNotMount => Ok(()),
        }
    });
    let data = futures::future::join_all(data);
    let (binary_data, data) = futures::future::join(binary_data, data).await;
    binary_data
        .into_iter()
        .chain(data)
        .collect::<tokio::io::Result<_>>()?;

    Ok(VolumeType::ConfigMap)
}
