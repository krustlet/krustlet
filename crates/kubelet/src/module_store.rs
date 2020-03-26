use crate::{ImageClient, Reference};
use async_trait::async_trait;
use tokio::sync::Mutex;

use std::path::{Path, PathBuf};
use std::sync::Arc;

#[async_trait]
pub trait ModuleStore {
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;
    async fn store(&self, image_ref: &Reference, contents: Vec<u8>) -> anyhow::Result<()>;
}

pub struct FileModuleStore<C> {
    root_dir: PathBuf,
    client: Arc<Mutex<C>>,
}

impl<C> FileModuleStore<C> {
    pub fn new<T: AsRef<Path>>(client: C, root_dir: T) -> Self {
        Self {
            root_dir: root_dir.as_ref().into(),
            client: Arc::new(Mutex::new(client)),
        }
    }

    fn pull_path(&self, r: &Reference) -> PathBuf {
        self.root_dir
            .join(r.registry())
            .join(r.repository())
            .join(r.tag())
    }

    fn pull_file_path(&self, r: &Reference) -> PathBuf {
        self.pull_path(r).join("module.wasm")
    }
}

#[async_trait]
impl<C: ImageClient + Sync + Send> ModuleStore for FileModuleStore<C> {
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        if !path.exists() {
            self.client.lock().await.pull(image_ref, self).await?;
        }
        Ok(tokio::fs::read(path).await?)
    }

    async fn store(&self, image_ref: &Reference, contents: Vec<u8>) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(self.pull_path(image_ref)).await?;
        let path = self.pull_file_path(image_ref);
        tokio::fs::write(&path, contents).await?;
        Ok(())
    }
}

impl<C> Clone for FileModuleStore<C> {
    fn clone(&self) -> Self {
        Self {
            root_dir: self.root_dir.clone(),
            client: self.client.clone(),
        }
    }
}
