//! A module for use in managing volumes in providers. Use of this module is not
//! mandatory to create a Provider, but it does provide common implementation
//! logic for supported volume providers.
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use k8s_openapi::api::core::v1::KeyToPath;
use k8s_openapi::api::core::v1::{PersistentVolumeClaim, Secret, Volume as KubeVolume};
use kube::api::Api;
use tracing::{debug, error};

use crate::plugin_watcher::PluginRegistry;
use crate::pod::Pod;

mod configmap;
mod hostpath;
mod persistentvolumeclaim;
mod secret;

/// type of volume
#[derive(Debug)]
pub enum VolumeType {
    /// configmap volume
    ConfigMap,
    /// secret volume
    Secret,
    /// PVC volume with an optional staging path if the volume supports stage/unstage
    PersistentVolumeClaim(Option<PathBuf>),
    /// hostpath volume
    HostPath,
}

/// A trait that can be implemented for something that can be mounted as a volume inside of a Pod
#[async_trait::async_trait]
pub trait Mountable {
    /// Mounts the object inside of the given base_path directory. Generally it will be mounted at a
    /// directory that is the base_path + the name of the volume
    async fn mount(&mut self, base_path: &Path) -> anyhow::Result<Ref>;

    // TODO(thomastaylor312): I don't like having to pass the path again, but that is how the
    // current system works. Perhaps we should keep the `Mountable` part in memory instead of a Ref,
    // but that is a separate refactor
    /// Unmounts the object from the given base_path
    async fn unmount(&mut self, base_path: &Path) -> anyhow::Result<()>;
}

/// A smart wrapper around the location of a volume on the host system. If this
/// is a ConfigMap or Secret volume, dropping this reference will clean up the
/// temporary volume. [AsRef] and [std::ops::Deref] are implemented for this
/// type so you can still use it like a normal PathBuf
#[derive(Debug)]
pub struct Ref {
    host_path: PathBuf,
    volume_type: VolumeType,
}

impl Ref {
    /// Resolves the volumes for a pod, including preparing temporary
    /// directories containing the contents of secrets and configmaps. Returns a
    /// HashMap of volume names to a PathBuf for the directory where the volume
    /// is mounted
    pub async fn volumes_from_pod(
        volume_dir: &Path,
        pod: &Pod,
        client: &kube::Client,
        plugin_registry: Option<Arc<PluginRegistry>>,
    ) -> anyhow::Result<HashMap<String, Self>> {
        let base_path = volume_dir.join(pod_dir_name(pod));
        let path: &Path = base_path.as_ref();
        tokio::fs::create_dir_all(&base_path).await?;
        let volumes = get_mountable_pod_volumes(pod, client, plugin_registry)
            .await?
            .into_iter()
            .map(|(k, mut m)| async move { Ok((k, m.mount(path).await?)) });
        futures::future::join_all(volumes)
            .await
            .into_iter()
            .collect::<anyhow::Result<_>>()
    }

    /// Unmounts any volumes mounted to the pod. Usually called when dropping
    /// the pod out of scope.
    pub async fn unmount_volumes_from_pod(
        volume_dir: &Path,
        pod: &Pod,
        client: &kube::Client,
        plugin_registry: Option<Arc<PluginRegistry>>,
    ) -> anyhow::Result<()> {
        let base_path = volume_dir.join(pod_dir_name(pod));
        let path: &Path = base_path.as_ref();
        let volumes = get_mountable_pod_volumes(pod, client, plugin_registry)
            .await?
            .into_iter()
            .map(|(_, mut m)| async move { m.unmount(path).await });
        futures::future::join_all(volumes)
            .await
            .into_iter()
            .collect::<anyhow::Result<_>>()
    }
}

impl AsRef<PathBuf> for Ref {
    fn as_ref(&self) -> &PathBuf {
        &self.host_path
    }
}

impl Deref for Ref {
    type Target = PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.host_path
    }
}

impl Drop for Ref {
    fn drop(&mut self) {
        if matches!(self.volume_type, VolumeType::ConfigMap | VolumeType::Secret) {
            // TODO: Currently there is no way to do this async (though there is
            // an async destructors proposal)
            debug!(
                "deleting {:?} directory {:?}",
                self.volume_type, self.host_path
            );
            std::fs::remove_dir_all(&self.host_path).unwrap_or_else(|e| {
                error!(
                    "unable to delete directory {:?} on volume cleanup: {:?}",
                    self.host_path, e
                )
            });
        }
    }
}

fn pod_dir_name(pod: &Pod) -> String {
    format!("{}-{}", pod.name(), pod.namespace())
}

fn mount_setting_for(key: &str, items_to_mount: &Option<Vec<KeyToPath>>) -> ItemMount {
    match items_to_mount {
        None => ItemMount::MountAt(key.to_string()),
        Some(items) => ItemMount::from(
            items
                .iter()
                .find(|kp| kp.key == key)
                .map(|kp| kp.path.to_string()),
        ),
    }
}

enum ItemMount {
    MountAt(String),
    DoNotMount,
}

impl From<Option<String>> for ItemMount {
    fn from(option: Option<String>) -> Self {
        match option {
            None => ItemMount::DoNotMount,
            Some(path) => ItemMount::MountAt(path),
        }
    }
}

/// Converts a Vec of KubeVolumes into a Vec of Mountable Volumes
async fn get_mountable_pod_volumes(
    pod: &Pod,
    client: &kube::Client,
    plugin_registry: Option<Arc<PluginRegistry>>,
) -> anyhow::Result<Vec<(String, Box<dyn Mountable + Send>)>> {
    let zero_vec = Vec::with_capacity(0);
    let mountables = pod
        .volumes()
        .unwrap_or(&zero_vec)
        .iter()
        .map(|v| (v, plugin_registry.clone()))
        .map(|(vol, pr)| async move {
            Ok((
                vol.name.clone(),
                to_mountable(vol, pod.namespace(), client, pr).await?,
            ))
        });
    futures::future::join_all(mountables)
        .await
        .into_iter()
        .collect()
}

async fn to_mountable(
    vol: &KubeVolume,
    namespace: &str,
    client: &kube::Client,
    plugin_registry: Option<Arc<PluginRegistry>>,
) -> anyhow::Result<Box<dyn Mountable + Send>> {
    if let Some(cm) = &vol.config_map {
        let name = &cm
            .name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no configmap name was given"))?;
        Ok(Box::new(configmap::ConfigMapVolume::new(
            &vol.name,
            name,
            namespace,
            client.clone(),
            cm.items.clone(),
        )))
    } else if let Some(s) = &vol.secret {
        let name = &s
            .secret_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no secret name was given"))?;
        Ok(Box::new(secret::SecretVolume::new(
            &vol.name,
            name,
            namespace,
            client.clone(),
            s.items.clone(),
        )))
    } else if let Some(pvc_source) = &vol.persistent_volume_claim {
        Ok(Box::new(
            persistentvolumeclaim::PvcVolume::new(
                pvc_source,
                client.clone(),
                &vol.name,
                namespace,
                plugin_registry,
            )
            .await?,
        ))
    } else if let Some(hp) = &vol.host_path {
        Ok(Box::new(hostpath::HostPathVolume::new(hp)))
    } else {
        Err(anyhow::anyhow!(
            "Unsupported volume type. Currently supported types: ConfigMap, Secret, PersistentVolumeClaim, and HostPath"
        ))
    }
}
