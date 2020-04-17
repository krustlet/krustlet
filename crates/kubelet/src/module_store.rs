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
///     async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
///         match self.modules.get(image_ref) {
///             Some(bytes) => Ok(bytes.clone()),
///             None => todo!("Fetch the bytes from some sort of remore store (e.g., OCI Distribution)")
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait ModuleStore {
    /// Get a module's data given its image `Reference`.
    ///
    /// It is up to the implementation to establish caching and network fetching policies.
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;

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
            async move { Ok((container.name.clone(), self.get(&reference).await?)) }
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
        path.push(r.tag());
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
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        if !path.exists() {
            debug!(
                "Image ref '{:?}' doesn't exist on disk. Fetching remotely...",
                image_ref
            );
            let contents = self.client.lock().await.pull(image_ref).await?;
            self.store(image_ref, &contents).await?;
            return Ok(contents);
        }

        debug!("Fetching image ref '{:?}' from disk", image_ref);
        Ok(tokio::fs::read(path).await?)
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
