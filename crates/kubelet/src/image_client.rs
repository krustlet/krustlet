use async_trait::async_trait;

use oci_distribution::Reference;

#[async_trait]
pub trait ImageClient {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>>;
}

#[async_trait]
impl ImageClient for oci_distribution::Client {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>> {
        self.pull_image(image).await
    }
}
