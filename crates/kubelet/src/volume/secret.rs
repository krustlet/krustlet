use std::path::Path;

use k8s_openapi::api::core::v1::{KeyToPath, Secret};
use k8s_openapi::ByteString;

use super::*;

pub(crate) async fn populate(
    secret: Secret,
    path: &Path,
    items: &Option<Vec<KeyToPath>>,
) -> anyhow::Result<VolumeType> {
    tokio::fs::create_dir_all(path).await?;
    let data = secret.data.unwrap_or_default();
    let data = data.into_iter().map(|(key, ByteString(data))| async move {
        match mount_setting_for(&key, items) {
            ItemMount::MountAt(mount_path) => {
                let file_path = path.join(mount_path);
                tokio::fs::write(file_path, &data).await
            }
            ItemMount::DoNotMount => Ok(()),
        }
    });
    futures::future::join_all(data)
        .await
        .into_iter()
        .collect::<tokio::io::Result<_>>()?;

    Ok(VolumeType::Secret)
}
