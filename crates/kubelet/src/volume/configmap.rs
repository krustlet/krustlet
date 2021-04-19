use std::path::Path;

use k8s_openapi::api::core::v1::{ConfigMap, KeyToPath};
use k8s_openapi::ByteString;

use super::*;

pub struct ConfigMapVolume {
    vol_name: String,
    cm_name: String,
    client: kube::Api<ConfigMap>,
    items: Option<Vec<KeyToPath>>,
}

impl ConfigMapVolume {
    pub fn new(
        vol_name: &str,
        cm_name: &str,
        namespace: &str,
        client: kube::Client,
        items: Option<Vec<KeyToPath>>,
    ) -> Self {
        ConfigMapVolume {
            vol_name: vol_name.to_owned(),
            cm_name: cm_name.to_owned(),
            client: Api::namespaced(client, namespace),
            items,
        }
    }
}

#[async_trait::async_trait]
impl Mountable for ConfigMapVolume {
    async fn mount(&mut self, base_path: &Path) -> anyhow::Result<Ref> {
        let config_map = self.client.get(&self.cm_name).await?;
        let path = base_path.join(&self.vol_name);
        tokio::fs::create_dir_all(&path).await?;

        let binary_data = config_map.binary_data.unwrap_or_default();
        let binary_data = binary_data
            .into_iter()
            .filter_map(
                |(key, ByteString(data))| match mount_setting_for(&key, &self.items) {
                    ItemMount::MountAt(mount_path) => Some((path.join(mount_path), data)),
                    ItemMount::DoNotMount => None,
                },
            )
            .map(|(file_path, data)| async move { tokio::fs::write(file_path, &data).await });
        let binary_data = futures::future::join_all(binary_data);

        let data = config_map.data.unwrap_or_default();
        let data = data
            .into_iter()
            .filter_map(|(key, data)| match mount_setting_for(&key, &self.items) {
                ItemMount::MountAt(mount_path) => Some((path.join(mount_path), data)),
                ItemMount::DoNotMount => None,
            })
            .map(|(file_path, data)| async move { tokio::fs::write(file_path, &data).await });
        let data = futures::future::join_all(data);

        let (binary_data, data) = futures::future::join(binary_data, data).await;
        binary_data
            .into_iter()
            .chain(data)
            .collect::<tokio::io::Result<_>>()?;

        Ok(Ref {
            host_path: path,
            volume_type: VolumeType::ConfigMap,
        })
    }

    async fn unmount(&mut self, _base_path: &Path) -> anyhow::Result<()> {
        // Unmounting is handled with the external ref type
        Ok(())
    }
}
