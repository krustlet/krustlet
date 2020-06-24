//! Client for fetching container modules from OCI
use async_trait::async_trait;
use oci_distribution::client::ImageData;

use oci_distribution::Reference;

/// An image client capable of fetching images from a storage location
#[async_trait]
pub trait Client {
    /// Fetch the image data and, if available, image digest for the given image
    /// reference from a storage location.
    ///
    /// # Example
    /// ```rust
    /// use async_trait::async_trait;
    /// use kubelet::store::oci::Client;
    /// use oci_distribution::Reference;
    /// use oci_distribution::client::ImageData;
    ///
    /// struct InMemoryClient(std::collections::HashMap<Reference, ImageData>);
    ///
    /// #[async_trait]
    /// impl Client for InMemoryClient {
    ///     async fn pull(&mut self, image_ref: &Reference) -> anyhow::Result<ImageData> {
    ///         let image_data = self
    ///             .0
    ///             .get(image_ref)
    ///             .ok_or(anyhow::anyhow!("Couldn't find image"))?;
    ///         Ok(image_data.clone())
    ///     }
    /// }
    /// ```
    async fn pull(&mut self, image_ref: &Reference) -> anyhow::Result<ImageData>;

    /// Fetch the digest for the given image reference from a storage location.
    ///
    /// The default implementation pulls the image data and digest, and returns
    /// the digest. This is inefficient for most real-world clients, and so should
    /// be overridden.
    async fn fetch_digest(&mut self, image_ref: &Reference) -> anyhow::Result<String> {
        let image_data = self.pull(image_ref).await?;
        image_data
            .digest
            .ok_or_else(|| anyhow::anyhow!("image {} does not have a digest", image_ref))
    }
}

#[async_trait]
impl Client for oci_distribution::Client {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<ImageData> {
        self.pull_image(image).await
    }

    async fn fetch_digest(&mut self, image: &Reference) -> anyhow::Result<String> {
        self.fetch_manifest_digest(image).await
    }
}
