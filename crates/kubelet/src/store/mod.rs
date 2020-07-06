//! `store` contains logic around fetching and storing modules.
pub mod composite;
pub mod fs;
pub mod oci;

use oci_distribution::client::ImageData;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;

use async_trait::async_trait;
use log::debug;
use oci_distribution::Reference;

use crate::container::PullPolicy;
use crate::pod::Pod;
use crate::store::oci::Client;

/// A store of container modules.
///
/// This provides the ability to get a module's bytes given an image [`Reference`].
///
/// # Example
///  ```rust
/// use async_trait::async_trait;
/// use oci_distribution::Reference;
/// use kubelet::container::PullPolicy;
/// use kubelet::store::Store;
/// use std::collections::HashMap;
///
/// struct InMemoryStore {
///     modules: HashMap<Reference, Vec<u8>>,
/// };
///
/// #[async_trait]
/// impl Store for InMemoryStore {
///     async fn get(&self, image_ref: &Reference, pull_policy: PullPolicy) -> anyhow::Result<Vec<u8>> {
///         match pull_policy {
///             PullPolicy::Never => (),
///             _ => todo!("Implement support for pull policies"),
///         }
///         match self.modules.get(image_ref) {
///             Some(bytes) => Ok(bytes.clone()),
///             None => todo!("Fetch the bytes from some sort of remore store (e.g., OCI Distribution)")
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait Store: Sync {
    /// Get a module's data given its image `Reference`.
    async fn get(&self, image_ref: &Reference, pull_policy: PullPolicy) -> anyhow::Result<Vec<u8>>;

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
        // let containers = pod.containers();
        // let container_module_futures = containers.iter().map(move |container| {
        //     let reference = container
        //         .image()
        //         .expect("Could not parse image.")
        let init_containers = pod.init_containers();
        let containers = pod.containers();
        let all_containers = init_containers.iter().chain(containers.iter());
        let container_module_futures = all_containers.map(move |container| {
            let reference = container
                .image()
                .expect("Could not parse image.")
                .expect("FATAL ERROR: container must have an image");
            let pull_policy = container
                .effective_pull_policy()
                .expect("Could not identify pull policy.");
            async move {
                Ok((
                    container.name().to_string(),
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

/// A `Store` implementation which obtains module data from remote registries
/// but caches it in local storage.
pub struct LocalStore<S: Storer, C: Client> {
    storer: Arc<RwLock<S>>,
    client: Arc<Mutex<C>>,
}

impl<S: Storer, C: Client> LocalStore<S, C> {
    async fn pull(&self, image_ref: &Reference) -> anyhow::Result<()> {
        debug!("Pulling image ref '{:?}' from registry", image_ref);
        let image_data = self.client.lock().await.pull(image_ref).await?;
        self.storer
            .write()
            .await
            .store(image_ref, image_data)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl<S: Storer + Sync + Send, C: Client + Sync + Send> Store for LocalStore<S, C> {
    async fn get(&self, image_ref: &Reference, pull_policy: PullPolicy) -> anyhow::Result<Vec<u8>> {
        match pull_policy {
            PullPolicy::IfNotPresent => {
                if !self.storer.read().await.is_present(image_ref).await {
                    self.pull(image_ref).await?
                }
            }
            PullPolicy::Always => {
                let digest = self.client.lock().await.fetch_digest(image_ref).await?;
                let already_got_with_digest = self
                    .storer
                    .read()
                    .await
                    .is_present_with_digest(image_ref, digest)
                    .await;
                if !already_got_with_digest {
                    self.pull(image_ref).await?
                }
            }
            PullPolicy::Never => (),
        };

        self.storer.read().await.get_local(image_ref).await
    }
}

/// A backing store for the `LocalStore` implementation of `Store`. The Storer
/// handles local I/O for module data and acts as a cache implementation.
#[async_trait]
pub trait Storer {
    /// Saves a module's data into the backing store indexed by its image `Reference`.
    async fn store(&mut self, image_ref: &Reference, image_data: ImageData) -> anyhow::Result<()>;

    /// Get a module's data from the backing store given its image `Reference`.
    ///
    /// The implementation must fail if the image is not present
    /// locally. `Storer` handles only reading and writing its own backing store;
    /// remote fetch is handled at the `Store` level.
    async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;

    /// Whether the specified module is already present in the backing store.
    async fn is_present(&self, image_ref: &Reference) -> bool;

    /// Whether the specified module is already present in the backing store with the specified digest.
    async fn is_present_with_digest(&self, image_ref: &Reference, digest: String) -> bool;
}
