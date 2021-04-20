use k8s_openapi::api::core::v1::HostPathVolumeSource;

use super::*;

pub struct HostPathVolume {
    host_path: PathBuf,
}

impl HostPathVolume {
    pub fn new(source: &HostPathVolumeSource) -> Self {
        HostPathVolume {
            host_path: PathBuf::from(&source.path),
        }
    }
}

#[async_trait::async_trait]
impl Mountable for HostPathVolume {
    async fn mount(&mut self, _base_path: &Path) -> anyhow::Result<Ref> {
        // Check the the directory exists on the host
        tokio::fs::metadata(&self.host_path).await?;
        Ok(Ref {
            host_path: self.host_path.clone(),
            volume_type: VolumeType::HostPath,
        })
    }

    async fn unmount(&mut self, _base_path: &Path) -> anyhow::Result<()> {
        // Not needed here as it is a host path
        Ok(())
    }
}
