use std::collections::BTreeMap;
use std::path::Path;

use k8s_openapi::{
    api::core::v1::{
        DownwardAPIVolumeFile, ObjectFieldSelector, ResourceFieldSelector, Volume as KubeVolume,
    },
    apimachinery::pkg::api::resource::Quantity as KubeQuantity
};
use tracing::warn;

use crate::container::Container;

use super::*;
/// A type that can manage a Downward API volume with mounting and unmounting support
pub struct DownwardApiVolume {
    vol_name: String,
    pod: Pod,
    items: Vec<DownwardAPIVolumeFile>,
    mounted_path: Option<PathBuf>,
}

impl DownwardApiVolume {
    /// Creates a new Downward API volume from a Kubernetes volume object. Passing a non-Downward
    /// API volume type will result in an error
    pub fn new(vol: &KubeVolume, pod: Pod) -> anyhow::Result<Self> {
        let da_source = vol.downward_api.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Called a Downward API volume constructor with a non-Downward API volume"
            )
        })?;
        Ok(DownwardApiVolume {
            vol_name: vol.name.clone(),
            pod,
            items: da_source.items.clone(),
            mounted_path: None,
        })
    }

    /// Returns the path where the volume is mounted on the host. Will return `None` if the volume
    /// hasn't been mounted yet
    pub fn get_path(&self) -> Option<&Path> {
        self.mounted_path.as_deref()
    }

    /// Mounts the Downward API volume in the given directory. The actual path will be
    /// $BASE_PATH/$VOLUME_NAME
    pub async fn mount(&mut self, base_path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = base_path.as_ref().join(&self.vol_name);
        tokio::fs::create_dir_all(&path).await?;

        // Mount field refs
        let field_refs = self
            .items
            .iter()
            .filter_map(|d| {
                d.field_ref
                    .as_ref()
                    .map(|f| (path.join(&d.path), data_from_field_ref(f, &self.pod)))
            })
            .map(|(p, res)| async move {
                let data = res?;
                tokio::fs::write(p, &data).await.map_err(|e| e.into())
            });
        let field_refs = futures::future::join_all(field_refs);

        let containers = self.pod.containers();
        let resource_refs = self
            .items
            .iter()
            .filter_map(|d| {
                d.resource_field_ref
                    .as_ref()
                    .map(|f| (path.join(&d.path), data_from_resource_ref(f, &containers)))
            })
            .map(|(p, res)| async move {
                let data = res?;
                tokio::fs::write(p, &data).await.map_err(|e| e.into())
            });
        let resource_refs = futures::future::join_all(resource_refs);

        let (field_refs, resource_refs) = futures::future::join(field_refs, resource_refs).await;
        field_refs
            .into_iter()
            .chain(resource_refs)
            .collect::<anyhow::Result<_>>()?;

        // Set directory to read-only.
        let mut perms = tokio::fs::metadata(&path).await?.permissions();
        perms.set_readonly(true);
        tokio::fs::set_permissions(&path, perms).await?;

        // Update the mounted directory
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
                tokio::task::spawn_blocking(|| remove_dir_all::remove_dir_all(p)).await??;

                #[cfg(target_family = "unix")]
                tokio::fs::remove_dir_all(p).await?;
            }
            None => {
                warn!("Attempted to unmount Downward API directory that wasn't mounted, this generally shouldn't happen");
            }
        }
        Ok(())
    }
}

/// Generates the data for the given field ref. This handling is a little bit different from the
/// environment variables version of the downward API because there are other fields allowed in the
/// environment variables and you can also get all of the labels in a single file using the volume.
/// See:
/// https://kubernetes.io/docs/tasks/inject-data-application/downward-api-volume-expose-pod-information/#capabilities-of-the-downward-api
fn data_from_field_ref(field_ref: &ObjectFieldSelector, pod: &Pod) -> anyhow::Result<Vec<u8>> {
    let (first, path) = field_ref.field_path.split_once('.').ok_or_else(|| {
        anyhow::anyhow!(
            "Path {} should be of the format metadata.<KEY_NAME>. No '.' separator was found",
            field_ref.field_path
        )
    })?;
    if first != "metadata" {
        anyhow::bail!(
            "Downward API access for volumes is only allowed with 'metadata'. Found {}",
            first
        );
    }

    // Label/annotation keys do not allow braces, so if they exist, splitting at the opening brace
    // should be valid or at least result in an invalid key if used maliciously
    let has_start_brace = path.contains('[');
    let has_end_brace = path.ends_with(']');
    let is_label_or_annotation = path == "labels" || path == "annotations";
    // Brief overview of what is being checked here as a precaution (since I think these fields are checked for validity by the API, but I am not sure):
    // 1. If the path contains any braces and isn't for labels or annotations, error
    // 2. If the path is missing a start or end brace (but has at least one) and is an annotation or label, error
    // 3. If all the braces are valid and it is a label or annotation, parse the key
    // 4. Otherwise, treat as a normal path
    let (path, key) = if (has_start_brace || has_end_brace) && !is_label_or_annotation {
        anyhow::bail!("Invalid path syntax, only 'labels' or 'annotations' can have key lookup (e.g. metadata.labels['mylabel'])")
    } else if (!has_start_brace ^ !has_end_brace) && is_label_or_annotation {
        anyhow::bail!(
            "Invalid path syntax, missing starting or ending brace for label/annotation key lookup"
        );
    } else if has_start_brace && has_end_brace && is_label_or_annotation {
        // We can unwrap here because we already validated there is a start brace
        let (path, rest) = path.split_once('[').unwrap();
        // Trim off the remaining expected characters
        // TODO: Do we want to validate open and close single quotes here?
        let trim_pat: &[_] = &['\'', ']'];
        let key = rest.trim_matches(trim_pat);
        (path, Some(key))
    } else {
        (path, None)
    };

    // Now grab the data
    Ok(match (path, key) {
        ("name", None) => pod.name().as_bytes().to_vec(),
        ("namespace", None) => pod.namespace().as_bytes().to_vec(),
        ("uid", None) => pod.pod_uid().as_bytes().to_vec(),
        ("labels", None) => btree_to_data(pod.labels()),
        ("labels", Some(key)) => pod
            .labels()
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("Key {} was not found in labels", key))?
            .as_bytes()
            .to_vec(),
        ("annotations", None) => btree_to_data(pod.annotations()),
        ("annotations", Some(key)) => pod
            .annotations()
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("Key {} was not found in annotations", key))?
            .as_bytes()
            .to_vec(),
        _ => anyhow::bail!("Field ref {} does not exist", field_ref.field_path),
    })
}

/// Converts an annotation or labels to a per-line printing with the label and value
fn btree_to_data(data: &BTreeMap<String, String>) -> Vec<u8> {
    data.iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, v))
        .collect::<Vec<String>>()
        .join("\n")
        .into_bytes()
}

fn data_from_resource_ref(
    resource_ref: &ResourceFieldSelector,
    containers: &[Container],
) -> anyhow::Result<Vec<u8>> {
    let (kind, path) = resource_ref.resource.split_once('.').ok_or_else(|| {
        anyhow::anyhow!(
            "Path {} should be of the format requests.<KEY_NAME>. No '.' separator was found",
            resource_ref.resource
        )
    })?;

    if kind != "requests" && kind != "limits" {
        anyhow::bail!(
            "Downward API resource ref only supports 'requests' and 'limits'. Found {}",
            kind
        );
    }

    let empty_string = String::new();
    let container_name = resource_ref
        .container_name
        .as_ref()
        .unwrap_or(&empty_string);
    let container = containers
        .iter()
        .find(|c| c.name() == container_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Container with name of {} does not exist in the pod",
                container_name
            )
        })?;
    let resources = container
        .resources()
        .ok_or_else(|| anyhow::anyhow!("No resources were found on the container"))?;
    let empty_quantity = KubeQuantity::default();
    Ok(match (kind, path) {
        ("requests", "cpu") => calculate_value(
            resources
                .requests
                .get("cpu")
                .ok_or_else(|| anyhow::anyhow!("A CPU request value was not found"))?,
            resource_ref.divisor.as_ref(),
        ),
        ("requests", "memory") => calculate_value(
            resources
                .requests
                .get("memory")
                .ok_or_else(|| anyhow::anyhow!("A memory request value was not found"))?,
            resource_ref.divisor.as_ref(),
        ),
        // TODO(thomastaylor312): According to the docs, if a limit is not specified, we should
        // default to the node allocatable value for CPU and memory. We have no easy way to access
        // that here, so for now we are just defaulting
        ("limits", "cpu") => calculate_value(
            resources.limits.get("cpu").unwrap_or(&empty_quantity),
            resource_ref.divisor.as_ref(),
        ),
        ("limits", "memory") => calculate_value(
            resources.limits.get("memory").unwrap_or(&empty_quantity),
            resource_ref.divisor.as_ref(),
        ),
        _ => anyhow::bail!("Resource ref {} does not exist", resource_ref.resource),
    })
}

fn calculate_value(data: &KubeQuantity, divisor: Option<&KubeQuantity>) -> Vec<u8> {
    // NOTE: The docs seem to indicate that at least basic verification of the quantity is done
    // during deserialization, but the code doesn't seem to reflect that, so we are performing at
    // least some of that validation here
    todo!()
}


