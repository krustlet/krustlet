use k8s_openapi::api::core::v1::Volume as KubeVolume;

use super::*;

/// A type that can manage a HostPath volume with mounting and unmounting support
pub struct HostPathVolume {
    host_path: PathBuf,
}

impl HostPathVolume {
    /// Creates a new HostPath volume from a Kubernetes volume object. Passing a non-HostPath volume
    /// type will result in an error
    pub fn new(vol: &KubeVolume) -> anyhow::Result<Self> {
        let source = vol.host_path.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Called a HostPath volume constructor with a non-HostPath volume")
        })?;
        Ok(HostPathVolume {
            host_path: PathBuf::from(&source.path),
        })
    }

    /// Returns the hostpath specified by the volume config
    pub fn get_path(&self) -> Option<&Path> {
        Some(self.host_path.as_path())
    }

    /// Mounts the configured host path volume. This just checks that the directory exists
    pub async fn mount(&mut self) -> anyhow::Result<()> {
        // Check the the directory exists on the host
        tokio::fs::metadata(&self.host_path).await?;
        Ok(())
    }
}
