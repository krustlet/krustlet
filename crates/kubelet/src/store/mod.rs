//! `store` contains logic around fetching and storing modules.
pub mod oci;

use std::collections::HashMap;
use std::convert::TryFrom;

use async_trait::async_trait;
use log::debug;
use oci_distribution::Reference;

use crate::pod::Pod;

/// A store of container modules.
///
/// This provides the ability to get a module's bytes given an image [`Reference`].
///
/// # Example
///  ```rust
/// use async_trait::async_trait;
/// use oci_distribution::Reference;
/// use kubelet::store::Store;
/// use std::collections::HashMap;
///
/// struct InMemoryStore {
///     modules: HashMap<Reference, Vec<u8>>,
/// };
///
/// #[async_trait]
/// impl Store for InMemoryStore {
///     async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
///         match self.modules.get(image_ref) {
///             Some(bytes) => Ok(bytes.clone()),
///             None => todo!("Fetch the bytes from some sort of remore store (e.g., OCI Distribution)")
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait Store {
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
