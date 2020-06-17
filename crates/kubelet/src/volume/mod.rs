//! A module for use in managing volumes in providers. Use of this module is not mandatory to create
//! a Provider, but it does provide common implementation logic for supported volume providers.
use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;

use k8s_openapi::api::core::v1::Volume as KubeVolume;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use k8s_openapi::ByteString;
use kube::api::Api;
use log::{debug, error};

use crate::pod::Pod;

#[derive(Debug)]
enum Type {
    ConfigMap,
    Secret,
    HostPath,
}

/// A smart wrapper around the location of a volume on the host system. If this is a ConfigMap or
/// Secret volume, dropping this reference will clean up the temporary volume. [AsRef] and
/// [std::ops::Deref] are implemented for this type so you can still use it like a normal PathBuf
#[derive(Debug)]
pub struct Ref {
    host_path: PathBuf,
    volume_type: Type,
}

impl Ref {
    /// Resolves the volumes for a pod, including preparing temporary directories containing the
    /// contents of secrets and configmaps. Returns a HashMap of volume names to a PathBuf for the
    /// directory where the volume is mounted
    pub async fn volumes_from_pod(
        volume_dir: &PathBuf,
        pod: &Pod,
        client: &kube::Client,
    ) -> anyhow::Result<HashMap<String, Self>> {
        let base_path = volume_dir.join(pod_dir_name(pod));
        tokio::fs::create_dir_all(&base_path).await?;
        if let Some(vols) = pod.volumes() {
            let volumes = vols.iter().map(|v| {
                let mut host_path = base_path.clone();
                host_path.push(&v.name);
                async move {
                    let volume_type = configure(v, pod.namespace(), client, &host_path).await?;
                    Ok((
                        v.name.to_owned(),
                        // Every other volume type should mount to the given host_path except for a
                        // hostpath volume type. So we need to handle that special case here
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
        if matches!(self.volume_type, Type::ConfigMap | Type::Secret) {
            // TODO: Currently there is no way to do this async (though there is an async destructors proposal)
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

/// This is a gnarly function to check all of the supported data members of the Volume struct.
/// Because it isn't a HashMap, we need to check all fields individually
async fn configure(
    vol: &KubeVolume,
    namespace: &str,
    client: &kube::Client,
    path: &PathBuf,
) -> anyhow::Result<Type> {
    if let Some(cm) = &vol.config_map {
        populate_from_config_map(
            &cm.name
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no configmap name was given"))?,
            namespace,
            client,
            path,
        )
        .await
    } else if let Some(s) = &vol.secret {
        populate_from_secret(
            &s.secret_name
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no secret name was given"))?,
            namespace,
            client,
            path,
        )
        .await
    } else if let Some(hostpath) = &vol.host_path {
        // Check the the directory exists on the host
        tokio::fs::metadata(&hostpath.path).await?;
        Ok(Type::HostPath)
    } else {
        Err(anyhow::anyhow!(
            "Unsupported volume type. Currently supported types: ConfigMap, Secret, and HostPath"
        ))
    }
}

async fn populate_from_secret(
    name: &str,
    namespace: &str,
    client: &kube::Client,
    path: &PathBuf,
) -> anyhow::Result<Type> {
    tokio::fs::create_dir_all(path).await?;
    let secret_client: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret = secret_client.get(name).await?;
    let data = secret.data.unwrap_or_default();
    let data = data.iter().map(|(key, ByteString(data))| async move {
        let file_path = path.join(key);
        tokio::fs::write(file_path, &data).await
    });
    futures::future::join_all(data)
        .await
        .into_iter()
        .collect::<tokio::io::Result<_>>()?;

    Ok(Type::Secret)
}

async fn populate_from_config_map(
    name: &str,
    namespace: &str,
    client: &kube::Client,
    path: &PathBuf,
) -> anyhow::Result<Type> {
    tokio::fs::create_dir_all(path).await?;
    let cm_client: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let config_map = cm_client.get(name).await?;
    let binary_data = config_map.binary_data.unwrap_or_default();
    let binary_data = binary_data.iter().map(|(key, data)| async move {
        let file_path = path.join(key);
        tokio::fs::write(file_path, &data.0).await
    });
    let binary_data = futures::future::join_all(binary_data);
    let data = config_map.data.unwrap_or_default();
    let data = data.iter().map(|(key, data)| async move {
        let file_path = path.join(key);
        tokio::fs::write(file_path, data).await
    });
    let data = futures::future::join_all(data);
    let (binary_data, data) = futures::future::join(binary_data, data).await;
    binary_data
        .into_iter()
        .chain(data)
        .collect::<tokio::io::Result<_>>()?;

    Ok(Type::ConfigMap)
}

fn pod_dir_name(pod: &Pod) -> String {
    format!("{}-{}", pod.name(), pod.namespace())
}
