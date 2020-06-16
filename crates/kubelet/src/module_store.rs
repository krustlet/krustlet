//! Stores of container module images
use crate::image_client::ImageClient;
use crate::pod::Pod;

use async_trait::async_trait;
use log::debug;
use oci_distribution::Reference;
use tokio::sync::Mutex;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Specifies how the store should check for module updates
#[derive(PartialEq, Debug)]
pub enum ModulePullPolicy {
    /// Always return the module as it currently appears in the
    /// upstream registry
    Always,
    /// Return the module as it is currently cached in the local store if
    /// present; fetch it from the upstream registry only if it it not
    /// present in the local store
    IfNotPresent,
    /// Never fetch the module from the upstream registry; if it is not
    /// available locally then return an error
    Never,
}

impl ModulePullPolicy {
    /// Parses a module pull policy from a Kubernetes ImagePullPolicy string
    pub fn parse(name: Option<String>) -> anyhow::Result<Option<ModulePullPolicy>> {
        match name {
            None => Ok(None),
            Some(n) => ModulePullPolicy::parse_str(&n[..]),
        }
    }

    fn parse_str(name: &str) -> anyhow::Result<Option<ModulePullPolicy>> {
        match name {
            "Always" => Ok(Some(Self::Always)),
            "IfNotPresent" => Ok(Some(Self::IfNotPresent)),
            "Never" => Ok(Some(Self::Never)),
            other => Err(anyhow::anyhow!("unrecognized pull policy {}", other)),
        }
    }
}

/// A store of container modules.
///
/// This provides the ability to get a module's bytes given an image [`Reference`].
///
/// # Example
///  ```rust
/// use async_trait::async_trait;
/// use oci_distribution::Reference;
/// use kubelet::module_store::ModuleStore;
/// use std::collections::HashMap;
///
/// struct InMemoryStore {
///     modules: HashMap<Reference, Vec<u8>>,
/// };
///
/// #[async_trait]
/// impl ModuleStore for InMemoryStore {
///     async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
///         match self.modules.get(image_ref) {
///             Some(bytes) => Ok(bytes.clone()),
///             None => todo!("Fetch the bytes from some sort of remore store (e.g., OCI Distribution)")
///         }
///     }
///
///     async fn is_present_by_ref(&self, image_ref: &Reference) -> bool {
///         self.modules.get(image_ref).is_some()
///     }
///
///     async fn is_present_by_digest(&self, digest: String) -> bool {
///         false
///     }
///
///     async fn pull(&self, image_ref: &Reference) -> anyhow::Result<()> {
///         Err(anyhow::anyhow!("InMemoryStore does not support registry pull"))
///     }
///
///     async fn resolve_registry_digest(&self, image_ref: &Reference) -> anyhow::Result<String> {
///         Err(anyhow::anyhow!("InMemoryStore does not support registry pull"))
///     }
/// }
/// ```
#[async_trait]
pub trait ModuleStore {
    /// Get a module's data given its image `Reference`.
    ///
    /// It is up to the implementation to establish caching policies.
    /// However, the implementation must fail if the image is not present
    /// locally.
    async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;

    /// Whether the specified module is already present in the store.
    async fn is_present_by_ref(&self, image_ref: &Reference) -> bool;

    /// Whether the module with the specified digest is already present in the store.
    async fn is_present_by_digest(&self, digest: String) -> bool;

    /// Pull a module from a remote source into the store.
    async fn pull(&self, image_ref: &Reference) -> anyhow::Result<()>;

    /// Get the digest of the specified module in its source registry.
    async fn resolve_registry_digest(&self, image_ref: &Reference) -> anyhow::Result<String>;

    /// Get a module's data given its image `Reference`
    async fn get(
        &self,
        image_ref: &Reference,
        pull_policy: Option<ModulePullPolicy>,
    ) -> anyhow::Result<Vec<u8>> {
        // Specification from https://kubernetes.io/docs/concepts/configuration/overview/#container-images):
        let effective_pull_policy = pull_policy.unwrap_or(match image_ref.tag() {
            Some("latest") | None => ModulePullPolicy::Always,
            _ => ModulePullPolicy::IfNotPresent,
        });

        match effective_pull_policy {
            ModulePullPolicy::IfNotPresent => {
                if !self.is_present_by_ref(image_ref).await {
                    self.pull(image_ref).await?
                }
            }
            ModulePullPolicy::Always => {
                if !self
                    .is_present_by_digest(self.resolve_registry_digest(image_ref).await?)
                    .await
                {
                    self.pull(image_ref).await?
                }
            }
            ModulePullPolicy::Never => (),
        };

        self.get_local(image_ref).await
    }

    /// Fetch all container modules for a given `Pod` storing the name of the
    /// container and the module's data as key/value pairs in a hashmap.
    ///
    /// This will fetch all of the container modules in parallel.
    ///
    /// # Panics
    ///
    /// This panics if any of the pod's containers do not have an image associated with them
    async fn fetch_pod_modules(&self, pod: &Pod) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        debug!(
            "Fetching all the container modules for pod '{}'",
            pod.name()
        );
        // Fetch all of the container modules in parallel
        let container_module_futures = pod.containers().iter().map(move |container| {
            let image = container
                .image
                .clone()
                .expect("FATAL ERROR: container must have an image");
            let reference = Reference::try_from(image).unwrap();
            let pull_policy = ModulePullPolicy::parse(container.image_pull_policy.clone()).unwrap();
            async move {
                Ok((
                    container.name.clone(),
                    self.get(&reference, pull_policy).await?,
                ))
            }
        });

        // Collect the container modules into a HashMap for quick lookup
        futures::future::join_all(container_module_futures)
            .await
            .into_iter()
            .collect()
    }
}

/// A module store that keeps modules cached on the file system
///
/// This type is generic over the type of Kubernetes client used
/// to fetch modules from a remote store. This client is expected
/// to be an [`ImageClient`]
pub struct FileModuleStore<C> {
    root_dir: PathBuf,
    client: Arc<Mutex<C>>,
}

impl<C> FileModuleStore<C> {
    /// Create a new `FileModuleStore`
    pub fn new<T: AsRef<Path>>(client: C, root_dir: T) -> Self {
        Self {
            root_dir: root_dir.as_ref().into(),
            client: Arc::new(Mutex::new(client)),
        }
    }

    fn pull_path(&self, r: &Reference) -> PathBuf {
        let mut path = self.root_dir.join(r.registry());
        path.push(r.repository());
        path.push(r.tag().unwrap_or("latest"));
        path
    }

    fn pull_file_path(&self, r: &Reference) -> PathBuf {
        self.pull_path(r).join("module.wasm")
    }

    async fn store(&self, image_ref: &Reference, contents: &[u8]) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(self.pull_path(image_ref)).await?;
        let path = self.pull_file_path(image_ref);
        tokio::fs::write(&path, contents).await?;
        Ok(())
    }
}

#[async_trait]
impl<C: ImageClient + Send> ModuleStore for FileModuleStore<C> {
    async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Image ref {} not available locally",
                image_ref
            ));
        }

        debug!("Fetching image ref '{:?}' from disk", image_ref);
        Ok(tokio::fs::read(path).await?)
    }

    async fn is_present_by_ref(&self, image_ref: &Reference) -> bool {
        let path = self.pull_file_path(image_ref);
        path.exists()
    }

    async fn is_present_by_digest(&self, _digest: String) -> bool {
        // panic!(format!(
        //     "TODO: is_present_by_digest not implemented while looking for {}",
        //     digest
        // ));
        todo!("is_present_by_digest not implemented");
    }

    async fn pull(&self, image_ref: &Reference) -> anyhow::Result<()> {
        debug!("Pulling image ref '{:?}' from registry", image_ref);
        let contents = self.client.lock().await.pull(image_ref).await?;
        self.store(image_ref, &contents).await?;
        Ok(())
    }

    async fn resolve_registry_digest(&self, image_ref: &Reference) -> anyhow::Result<String> {
        self.client.lock().await.fetch_digest(image_ref).await
    }
}

impl<C> Clone for FileModuleStore<C> {
    fn clone(&self) -> Self {
        Self {
            root_dir: self.root_dir.clone(),
            client: self.client.clone(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_can_parse_pull_policies() {
        assert_eq!(None, ModulePullPolicy::parse(None).unwrap());
        assert_eq!(
            ModulePullPolicy::Always,
            ModulePullPolicy::parse(Some("Always".to_owned()))
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            ModulePullPolicy::IfNotPresent,
            ModulePullPolicy::parse(Some("IfNotPresent".to_owned()))
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            ModulePullPolicy::Never,
            ModulePullPolicy::parse(Some("Never".to_owned()))
                .unwrap()
                .unwrap()
        );
        assert!(
            ModulePullPolicy::parse(Some("IfMoonMadeOfGreenCheese".to_owned())).is_err(),
            "Expected parse failure but didn't get one"
        );
    }
}
