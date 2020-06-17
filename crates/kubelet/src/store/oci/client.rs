//! Client for fetching container modules from OCI
use async_trait::async_trait;

use oci_distribution::Reference;

/// An image client capable of fetching images from a storage location
#[async_trait]
pub trait Client {
    /// Given a certain image reference pull the image data from a storage location
    ///
    /// # Example
    /// ```rust
    /// use async_trait::async_trait;
    /// use kubelet::store::oci::Client;
    /// use oci_distribution::Reference;
    ///
    /// struct InMemoryClient(std::collections::HashMap<Reference, Vec<u8>>);
    ///
    /// #[async_trait]
    /// impl Client for InMemoryClient {
    ///     async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>> {
    ///         let image = self
    ///             .0
    ///             .get(image)
    ///             .ok_or(anyhow::anyhow!("Couldn't find image"))?;
    ///         Ok(image.clone())
    ///     }
    /// }
    /// ```
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>>;
}

#[async_trait]
impl Client for oci_distribution::Client {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>> {
        self.pull_image(image).await
    }
}
