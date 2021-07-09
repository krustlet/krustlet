use std::path::Path;

use either::Either;
use k8s_openapi::api::authentication::v1::TokenRequest;
use k8s_openapi::api::core::v1::{Volume as KubeVolume, VolumeProjection};
use tracing::warn;

use super::*;

/// A type that can manage a Secret volume with mounting and unmounting support
pub struct ProjectedVolume {
    vol_name: String,
    volumes: Vec<super::VolumeRef>,
    service_accounts: Vec<ServiceAccountSource>,
    mounted_path: Option<PathBuf>,
}

struct ServiceAccountSource {
    file_name: String,
    service_account_name: String,
    client: kube::Api<TokenRequest>,
}

impl ServiceAccountSource {
    async fn mount(&mut self, base_path: impl AsRef<Path>) -> anyhow::Result<()> {
        todo!()
    }

    async fn unmount(&mut self) -> anyhow::Result<()> {
        todo!()
    }
}

impl ProjectedVolume {
    /// Creates a new Projected volume from a Kubernetes volume object. Passing a non-Projected
    /// volume type will result in an error. If any of the projected volume sources are a service
    /// account token, the name of the service account must be passed from the pod spec
    pub fn new(
        vol: &KubeVolume,
        namespace: &str,
        client: kube::Client,
        service_account_name: Option<&str>,
    ) -> anyhow::Result<Self> {
        let source = vol.projected.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Called a Projected volume constructor with a non-projected volume")
        })?;
        let mut volumes = Vec::new();
        let mut service_accounts = Vec::new();
        // TODO: DownwardAPI vols
        for s in source
            .sources
            .iter()
            .map(|proj| (client.clone(), proj))
            .map(|(c, proj)| to_volume_ref(c, namespace, service_account_name, proj))
            .collect::<anyhow::Result<Vec<Either<_, _>>>>()?
            .into_iter()
        {
            match s {
                Either::Left(v) => volumes.push(v),
                Either::Right(sa) => service_accounts.push(sa),
            }
        }
        Ok(ProjectedVolume {
            vol_name: vol.name.clone(),
            volumes,
            service_accounts,
            mounted_path: None,
        })
    }

    /// Returns the path where the volume is mounted on the host. Will return `None` if the volume
    /// hasn't been mounted yet
    pub fn get_path(&self) -> Option<&Path> {
        self.mounted_path.as_deref()
    }

    /// Mounts the Secret volume in the given directory. The actual path will be
    /// $BASE_PATH/$VOLUME_NAME
    pub async fn mount(&mut self, base_path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = base_path.as_ref().join(&self.vol_name);
        tokio::fs::create_dir_all(&path).await?;

        let vol_futures = self
            .volumes
            .iter_mut()
            .map(|v| (path.clone(), v))
            .map(|(p, v)| async move { v.mount(p).await });

        let sa_futures = self
            .service_accounts
            .iter_mut()
            .map(|s| (path.clone(), s))
            .map(|(p, s)| async move { s.mount(p).await });

        // Join together all of the futures and then collect any errors. We can't just chain
        // together the future iterators because they technically have different types
        let (res1, res2) = futures::future::join(
            futures::future::join_all(vol_futures),
            futures::future::join_all(sa_futures),
        )
        .await;
        res1.into_iter()
            .chain(res2.into_iter())
            .collect::<anyhow::Result<()>>()?;

        self.mounted_path = Some(path);

        Ok(())
    }

    /// Unmounts the directory, which removes all files. Calling `unmount` on a directory that
    /// hasn't been mounted will log a warning, but otherwise not error
    pub async fn unmount(&mut self) -> anyhow::Result<()> {
        match self.mounted_path.take() {
            Some(p) => {
                //although remove_dir_all crate could default to std::fs::remove_dir_all for unix family, we still prefer std::fs implemetation for unix
                #[cfg(target_family = "windows")]
                tokio::task::spawn_blocking(|| remove_dir_all::remove_dir_all(p)).await?;

                #[cfg(target_family = "unix")]
                tokio::fs::remove_dir_all(p).await?;
            }
            None => {
                warn!("Attempted to unmount ConfigMap directory that wasn't mounted, this generally shouldn't happen");
            }
        }
        Ok(())
    }
}

fn to_volume_ref(
    client: kube::Client,
    namespace: &str,
    service_account_name: Option<&str>,
    proj: &VolumeProjection,
) -> anyhow::Result<Either<super::VolumeRef, ServiceAccountSource>> {
    todo!()
}
