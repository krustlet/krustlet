use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use log::debug;
use oci_distribution::Reference;
use tokio::sync::Mutex;

use super::client::Client;
use crate::store::Store;

/// A module store that keeps modules cached on the file system
///
/// This type is generic over the type of Kubernetes client used
/// to fetch modules from a remote store. This client is expected
/// to be a [`Client`]
pub struct FileStore<C> {
    root_dir: PathBuf,
    client: Arc<Mutex<C>>,
}

impl<C> FileStore<C> {
    /// Create a new `FileStore`
    pub fn new<T: AsRef<Path>>(client: C, root_dir: T) -> Self {
        Self {
            root_dir: root_dir.as_ref().into(),
            client: Arc::new(Mutex::new(client)),
        }
    }

    fn pull_path(&self, r: &Reference) -> PathBuf {
        let mut path = self.root_dir.join(r.registry());
        path.push(r.repository());
        path.push(r.tag());
        path
    }

    fn pull_file_path(&self, r: &Reference) -> PathBuf {
        self.pull_path(r).join("module.wasm")
    }

    async fn store(&self, image_ref: &Reference, contents: &[u8]) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(self.pull_path(image_ref)).await?;
        let path = self.pull_file_path(image_ref);
        tokio::fs::write(&path, contents).await?;
        Ok(())
    }
}

#[async_trait]
impl<C: Client + Send> Store for FileStore<C> {
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        if !path.exists() {
            debug!(
                "Image ref '{:?}' doesn't exist on disk. Fetching remotely...",
                image_ref
            );
            let contents = self.client.lock().await.pull(image_ref).await?;
            self.store(image_ref, &contents).await?;
            return Ok(contents);
        }

        debug!("Fetching image ref '{:?}' from disk", image_ref);
        Ok(tokio::fs::read(path).await?)
    }
}

impl<C> Clone for FileStore<C> {
    fn clone(&self) -> Self {
        Self {
            root_dir: self.root_dir.clone(),
            client: self.client.clone(),
        }
    }
}
