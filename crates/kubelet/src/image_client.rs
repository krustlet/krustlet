//! Container module image clients that can fetch images from remote stores
use async_trait::async_trait;

use oci_distribution::Reference;

/// An image client capable of fetching images from a remote store
#[async_trait]
pub trait ImageClient {
    /// Given a certain image reference pull the image data from the remote store
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>>;
}

#[async_trait]
impl ImageClient for oci_distribution::Client {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>> {
        self.pull_image(image).await
    }
}
