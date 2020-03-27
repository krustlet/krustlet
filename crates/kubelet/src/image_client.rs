//! Container module image clients that can fetch images from remote stores
use async_trait::async_trait;

use oci_distribution::Reference;

/// An image client capable of fetching images from a remote store
#[async_trait]
pub trait ImageClient {
    /// Given a certain image reference pull the image data from the remote store
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
    ///             .clone()
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
