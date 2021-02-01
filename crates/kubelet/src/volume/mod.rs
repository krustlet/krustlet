//! A module for use in managing volumes in providers. Use of this module is not
//! mandatory to create a Provider, but it does provide common implementation
//! logic for supported volume providers.
use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use k8s_openapi::api::core::v1::KeyToPath;
use k8s_openapi::api::core::v1::{ConfigMap, PersistentVolumeClaim, Secret, Volume as KubeVolume};
use kube::api::Api;
use log::{debug, error};

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
    /// PVC volume
    PersistentVolumeClaim,
    /// hostpath volume
    HostPath,
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
        volume_dir: &PathBuf,
        pod: &Pod,
        client: &kube::Client,
        plugin_registry: Option<Arc<PluginRegistry>>,
    ) -> anyhow::Result<HashMap<String, Self>> {
        let base_path = volume_dir.join(pod_dir_name(pod));
        tokio::fs::create_dir_all(&base_path).await?;
        if let Some(vols) = pod.volumes() {
            let volumes = vols.iter().map(|v| {
                let mut host_path = base_path.clone();
                host_path.push(&v.name);
                let pr = plugin_registry.clone();
                async move {
                    let volume_type = configure(v, pod.namespace(), client, pr, &host_path).await?;
                    Ok((
                        v.name.to_owned(),
                        // Every other volume type should mount to the given
                        // host_path except for a hostpath volume type. So we
                        // need to handle that special case here
                        match &v.host_path {
                            Some(hostpath) => Ref {
                                host_path: PathBuf::from(&hostpath.path),
                                volume_type,
                            },
                            None => Ref {
                                host_path,
                                volume_type,
                            },
                        },
                    ))
                }
            });
            futures::future::join_all(volumes)
                .await
                .into_iter()
                .collect()
        } else {
            Ok(HashMap::default())
        }
    }

    /// Unmounts any volumes mounted to the pod. Usually called when dropping
    /// the pod out of scope.
    pub async fn unmount_volumes_from_pod(
        volume_dir: &PathBuf,
        pod: &Pod,
        client: &kube::Client,
        plugin_registry: Option<Arc<PluginRegistry>>,
    ) -> anyhow::Result<()> {
        if let Some(vols) = pod.volumes() {
            let base_path = volume_dir.join(pod_dir_name(pod));
            for vol in vols {
                if let Some(pvc_source) = &vol.persistent_volume_claim {
                    let vol_path = base_path.join(&vol.name);
                    persistentvolumeclaim::unpopulate(
                        pvc_source,
                        client,
                        pod.namespace(),
                        plugin_registry.clone(),
                        &vol_path,
                    )
                    .await?;
                }
            }
        }
        Ok(())
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

/// This is a gnarly function to check all of the supported data members of the
/// Volume struct. Because it isn't a HashMap, we need to check all fields
/// individually
async fn configure(
    vol: &KubeVolume,
    namespace: &str,
    client: &kube::Client,
    plugin_registry: Option<Arc<PluginRegistry>>,
    path: &PathBuf,
) -> anyhow::Result<VolumeType> {
    if let Some(cm) = &vol.config_map {
        let name = &cm
            .name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no configmap name was given"))?;
        let cm_client: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
        let config_map = cm_client.get(name).await?;
        configmap::populate(config_map, path, &cm.items).await
    } else if let Some(s) = &vol.secret {
        let name = &s
            .secret_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no secret name was given"))?;
        let secret_client: Api<Secret> = Api::namespaced(client.clone(), namespace);
        let secret = secret_client.get(name).await?;
        secret::populate(secret, path, &s.items).await
    } else if let Some(pvc_source) = &vol.persistent_volume_claim {
        persistentvolumeclaim::populate(pvc_source, client, namespace, plugin_registry, path).await
    } else if let Some(hp) = &vol.host_path {
        hostpath::populate(hp).await
    } else {
        Err(anyhow::anyhow!(
            "Unsupported volume type. Currently supported types: ConfigMap, Secret, PersistentVolumeClaim, and HostPath"
        ))
    }
}
