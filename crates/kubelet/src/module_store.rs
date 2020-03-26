use crate::{pod::Pod, ImageClient, Reference};
use async_trait::async_trait;
use tokio::sync::Mutex;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[async_trait]
pub trait ModuleStore {
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;

    // Fetch all container modules for a given `Pod` storing the name of the
    // container and the module's data as key/value pairs in a hashmap.
    async fn fetch_container_modules(&self, pod: &Pod) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        // Fetch all of the container modules in parallel
        let container_module_futures = pod.containers().iter().map(move |container| {
            let image = container
                .image
                .clone()
                .expect("FATAL ERROR: container must have an image");
            let reference = Reference::try_from(image).unwrap();
            async move { Ok((container.name.clone(), self.get(&reference).await?)) }
        });

        // Collect the container modules into a HashMap for quick lookup
        futures::future::join_all(container_module_futures)
            .await
            .into_iter()
            .collect()
    }
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
impl<C: ImageClient + Sync + Send> ModuleStore for FileModuleStore<C> {
    async fn get(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        if !path.exists() {
            let contents = self.client.lock().await.pull(image_ref).await?;
            self.store(image_ref, &contents).await?;
            return Ok(contents);
        }

        Ok(tokio::fs::read(path).await?)
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
