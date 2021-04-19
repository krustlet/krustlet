use std::path::Path;

use k8s_openapi::api::core::v1::{KeyToPath, Secret};
use k8s_openapi::ByteString;

use super::*;

pub struct SecretVolume {
    vol_name: String,
    sec_name: String,
    client: kube::Api<Secret>,
    items: Option<Vec<KeyToPath>>,
}

impl SecretVolume {
    pub fn new(
        vol_name: &str,
        sec_name: &str,
        namespace: &str,
        client: kube::Client,
        items: Option<Vec<KeyToPath>>,
    ) -> Self {
        SecretVolume {
            vol_name: vol_name.to_owned(),
            sec_name: sec_name.to_owned(),
            client: Api::namespaced(client, namespace),
            items,
        }
    }
}

#[async_trait::async_trait]
impl Mountable for SecretVolume {
    async fn mount(&mut self, base_path: &Path) -> anyhow::Result<Ref> {
        let secret = self.client.get(&self.sec_name).await?;
        let path = base_path.join(&self.vol_name);
        tokio::fs::create_dir_all(&path).await?;
        let data = secret.data.unwrap_or_default();
        // We could probably just move the data out of the option, but I don't know what the correct
        // behavior is from k8s point of view if something tries to mount a volume again
        let data = data
            .into_iter()
            .filter_map(
                |(key, ByteString(data))| match mount_setting_for(&key, &self.items) {
                    ItemMount::MountAt(mount_path) => Some((path.join(mount_path), data)),
                    ItemMount::DoNotMount => None,
                },
            )
            .map(|(file_path, data)| async move { tokio::fs::write(file_path, &data).await });
        futures::future::join_all(data)
            .await
            .into_iter()
            .collect::<tokio::io::Result<_>>()?;

        Ok(Ref {
            host_path: path,
            volume_type: VolumeType::Secret,
        })
    }

    async fn unmount(&mut self, _base_path: &Path) -> anyhow::Result<()> {
        // Unmounting handled by external Ref type
        Ok(())
    }
}
