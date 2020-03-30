//! Clients for fetching container module images from a storage location
//!
//! These clients are usually used together with some module store
//! in order to fetch module an image when the module store does not
//! contain it
use async_trait::async_trait;

use oci_distribution::Reference;

/// An image client capable of fetching images from a storage location
#[async_trait]
pub trait ImageClient {
    /// Given a certain image reference pull the image data from a storage location
    ///
    /// # Example
    /// ```rust
    /// use async_trait::async_trait;
    /// use kubelet::image_client::ImageClient;
    /// use oci_distribution::Reference;
    ///
    /// struct InMemoryClient(std::collections::HashMap<Reference, Vec<u8>>);
    ///
    /// #[async_trait]
    /// impl ImageClient for InMemoryClient {
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
impl ImageClient for oci_distribution::Client {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>> {
        self.pull_image(image).await
    }
}
