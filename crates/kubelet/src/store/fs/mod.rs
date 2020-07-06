//! `fs` implements fetching modules from the local file system.

use crate::store::composite::InterceptingStore;
use crate::store::{PullPolicy, Store};
use async_trait::async_trait;
use oci_distribution::Reference;
use std::path::PathBuf;

/// TODO
pub struct FileSystemStore {}

#[async_trait]
impl Store for FileSystemStore {
    async fn get(
        &self,
        image_ref: &Reference,
        _pull_policy: Option<PullPolicy>,
    ) -> anyhow::Result<Vec<u8>> {
        let path = PathBuf::from(image_ref.repository());
        Ok(tokio::fs::read(&path).await?)
    }
}

impl InterceptingStore for FileSystemStore {
    fn intercepts(&self, image_ref: &Reference) -> bool {
        image_ref.registry() == "fs"
    }
}
