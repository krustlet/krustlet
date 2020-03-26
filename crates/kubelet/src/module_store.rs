use crate::reference::Reference;
use async_trait::async_trait;

use std::path::{Path, PathBuf};

#[async_trait]
pub trait ModuleStore {
    async fn store(&self, image_ref: &Reference, contents: Vec<u8>) -> anyhow::Result<()>;
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;
}

#[derive(Clone)]
pub struct FileModuleStore {
    root_dir: PathBuf,
}

impl FileModuleStore {
    pub fn new(root_dir: &Path) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    pub(crate) fn pull_path(&self, r: &Reference) -> PathBuf {
        self.root_dir
            .join(r.registry())
            .join(r.repository())
            .join(r.tag())
    }

    pub fn pull_file_path(&self, r: &Reference) -> PathBuf {
        self.pull_path(r).join("module.wasm")
    }
}

#[async_trait]
impl ModuleStore for FileModuleStore {
    async fn store(&self, image_ref: &Reference, contents: Vec<u8>) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(self.pull_path(image_ref)).await?;
        let path = self.pull_file_path(image_ref);
        tokio::fs::write(&path, contents).await?;
        Ok(())
    }

    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        Ok(tokio::fs::read(path).await?)
    }
}
