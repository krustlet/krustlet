//! `fs` implements fetching modules from the local file system.

use crate::store::composite::InterceptingStore;
use crate::store::{PullPolicy, Store};
use async_trait::async_trait;
use oci_distribution::secrets::RegistryAuth;
use oci_distribution::Reference;
use std::path::PathBuf;

/// A `Store` which fetches modules only from the local filesystem,
/// not a remote registry. References must be of the form
/// fs/<path>, e.g. fs//wasm/mymodule.wasm or fs/./out/mymodule.wasm.
/// Version tags are ignored.
///
/// FileSystemStore can be composed with another Store to support hybrid retrieval -
/// typically a developer scenario where you want the application under
/// test to be your local build, but all other modules to be retrieved from
/// their production registries.
pub struct FileSystemStore {}

#[async_trait]
impl Store for FileSystemStore {
    async fn get(
        &self,
        image_ref: &Reference,
        _pull_policy: PullPolicy,
        _auth: &RegistryAuth,
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
