use async_trait::async_trait;

use crate::Reference;

#[async_trait]
pub trait ImageClient {
    async fn pull(&mut self, image: &Reference) -> anyhow::Result<Vec<u8>>;
}
