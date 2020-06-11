//! Clients for fetching container module images from a storage location
//!
//! These clients are usually used together with some module store
//! in order to fetch module an image when the module store does not
//! contain it
use async_trait::async_trait;
use digest::Digest;

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
    ///     async fn pull(&mut self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
    ///         let image = self
    ///             .0
    ///             .get(image_ref)
    ///             .ok_or(anyhow::anyhow!("Couldn't find image"))?;
    ///         Ok(image.clone())
    ///     }
    /// }
    /// ```
    async fn pull(&mut self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;

    /// Fetch the digest for the given image reference from a storage location.
    ///
    /// The default implementation pulls the entire image and calculates the digest;
    /// clients should provide an implementation that takes advantage of the storage
    /// location's pre-calculated digests if possible.
    async fn fetch_digest(&mut self, image_ref: &Reference) -> anyhow::Result<String> {
        let image = self.pull(image_ref).await?;
        let digest = sha2::Sha256::digest(&image);
        let digest_text = format!("sha256:{:x}", digest);
        Ok(digest_text)
    }
}

#[async_trait]
impl ImageClient for oci_distribution::Client {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>> {
        self.pull_image(image).await
    }

    async fn fetch_digest(&mut self, image: &Reference) -> anyhow::Result<String> {
        self.fetch_manifest_digest(image).await
    }
}
