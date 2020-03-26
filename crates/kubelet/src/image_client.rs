use async_trait::async_trait;

use crate::{ModuleStore, Reference};

#[async_trait]
pub trait ImageClient {
    async fn pull<T: ModuleStore + Send + Sync>(
        &mut self,
        image: &Reference,
        store: &T,
    ) -> anyhow::Result<()>;
}
