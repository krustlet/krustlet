use crate::store::Storer;
use oci_distribution::client::ImageData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use oci_distribution::Reference;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tracing::debug;

use super::client::Client;
use crate::store::LocalStore;

/// A module store that keeps modules cached on the file system
///
/// This type is generic over the type of client used
/// to fetch modules from a remote store. This client is expected
/// to be a [`Client`]
pub type FileStore<C> = LocalStore<FileStorer, C>;

impl<C: Client + Send> FileStore<C> {
    /// Create a new `FileStore`
    pub fn new<T: AsRef<Path>>(client: C, root_dir: T) -> Self {
        Self {
            storer: Arc::new(RwLock::new(FileStorer {
                root_dir: root_dir.as_ref().into(),
            })),
            client: Arc::new(Mutex::new(client)),
        }
    }
}

pub struct FileStorer {
    root_dir: PathBuf,
}

impl FileStorer {
    /// Create a new `FileStorer`
    pub fn new<T: AsRef<Path>>(root_dir: T) -> Self {
        Self {
            root_dir: root_dir.as_ref().into(),
        }
    }

    fn pull_path(&self, r: &Reference) -> PathBuf {
        let mut path = self.root_dir.join(r.registry());
        path.push(r.repository());
        path.push(r.tag().unwrap_or("latest"));
        path
    }

    fn pull_file_path(&self, r: &Reference) -> PathBuf {
        self.pull_path(r).join("module.wasm")
    }

    fn digest_file_path(&self, r: &Reference) -> PathBuf {
        self.pull_path(r).join("digest.txt")
    }
}

#[async_trait]
impl Storer for FileStorer {
    async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Image ref {} not available locally",
                image_ref
            ));
        }

        debug!(?image_ref, "Fetching image ref from disk");
        Ok(tokio::fs::read(path).await?)
    }
    async fn store(&mut self, image_ref: &Reference, image_data: ImageData) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(self.pull_path(image_ref)).await?;
        let digest_path = self.digest_file_path(image_ref);
        // We delete the digest file before writing the image file, rather
        // than simply overwriting the digest file after writing the image file.
        // This addresses failure modes where, for example, the image file
        // gets updated but the digest file write fails and the store ends
        // up associating the wrong digest with the file on disk.
        if digest_path.exists() {
            tokio::fs::remove_file(&digest_path).await?;
        }
        // FIXME: we need to determine the proper file path for each layer rather than assuming it's a single-layer image.
        let module_path = self.pull_file_path(image_ref);
        if image_data.layers.is_empty() {
            return Err(anyhow::anyhow!("No module layer present in image data"));
        }
        tokio::fs::write(&module_path, &image_data.layers[0].data).await?;
        if let Some(d) = image_data.digest {
            tokio::fs::write(&digest_path, d).await?;
        }
        Ok(())
    }

    async fn is_present(&self, image_ref: &Reference) -> bool {
        let path = self.pull_file_path(image_ref);
        path.exists()
    }

    async fn is_present_with_digest(&self, image_ref: &Reference, digest: String) -> bool {
        let path = self.digest_file_path(image_ref);
        path.exists() && file_content_is(path, digest).await
    }
}

impl<C: Client + Send> Clone for FileStore<C> {
    fn clone(&self) -> Self {
        Self {
            storer: self.storer.clone(),
            client: self.client.clone(),
        }
    }
}

async fn file_content_is(path: PathBuf, text: String) -> bool {
    match tokio::fs::read(path).await {
        Err(_) => false,
        Ok(content) => {
            let file_text = String::from_utf8_lossy(&content);
            file_text == text
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::container::PullPolicy;
    use crate::store::Store;
    use oci_distribution::client::{ImageData, ImageLayer};
    use oci_distribution::secrets::RegistryAuth;
    use std::collections::HashMap;
    use std::convert::TryFrom;
    use std::sync::RwLock;

    #[tokio::test]
    async fn can_parse_pull_policies() {
        assert_eq!(None, PullPolicy::parse(None).unwrap());
        assert_eq!(
            PullPolicy::Always,
            PullPolicy::parse(Some("Always")).unwrap().unwrap()
        );
        assert_eq!(
            PullPolicy::IfNotPresent,
            PullPolicy::parse(Some("IfNotPresent")).unwrap().unwrap()
        );
        assert_eq!(
            PullPolicy::Never,
            PullPolicy::parse(Some("Never")).unwrap().unwrap()
        );
        assert!(
            PullPolicy::parse(Some("IfMoonMadeOfGreenCheese")).is_err(),
            "Expected parse failure but didn't get one"
        );
    }

    #[derive(Clone)]
    struct FakeImageClient {
        images: Arc<RwLock<HashMap<String, ImageData>>>,
    }

    impl FakeImageClient {
        fn new(entries: Vec<(&'static str, Vec<u8>, &'static str)>) -> Self {
            let client = FakeImageClient {
                images: Default::default(),
            };
            for (name, content, digest) in entries {
                let mut images = client
                    .images
                    .write()
                    .expect("should be able to write to images");
                images.insert(
                    name.to_owned(),
                    ImageData {
                        layers: vec![ImageLayer::oci_v1(content)],
                        digest: Some(digest.to_owned()),
                    },
                );
            }
            client
        }

        fn update(&mut self, key: &str, content: Vec<u8>, digest: &str) {
            let mut images = self
                .images
                .write()
                .expect("should be able to write to images");
            images.insert(
                key.to_owned(),
                ImageData {
                    layers: vec![ImageLayer::oci_v1(content)],
                    digest: Some(digest.to_owned()),
                },
            );
        }
    }
    #[async_trait]
    impl Client for FakeImageClient {
        async fn pull(
            &mut self,
            image_ref: &Reference,
            _auth: &RegistryAuth,
        ) -> anyhow::Result<ImageData> {
            let images = self
                .images
                .read()
                .expect("should be able to read from images");
            match images.get(&image_ref.whole()) {
                Some(v) => Ok(v.clone()),
                None => Err(anyhow::anyhow!("error pulling module")),
            }
        }
    }

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.path).expect("Failed to remove temp directory");
        }
    }

    fn create_temp_dir() -> TemporaryDirectory {
        let os_temp_dir = std::env::temp_dir();
        let subdirectory = PathBuf::from(format!("krustlet-fms-tests-{}", uuid::Uuid::new_v4()));
        let path = os_temp_dir.join(subdirectory);
        std::fs::create_dir(&path).expect("Failed to create temp directory");
        TemporaryDirectory { path }
    }

    #[tokio::test]
    async fn file_module_store_can_pull_if_policy_if_not_present() -> anyhow::Result<()> {
        let fake_client = FakeImageClient::new(vec![("foo/bar:1.0", vec![1, 2, 3], "sha256:123")]);
        let fake_ref = Reference::try_from("foo/bar:1.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client, &scratch_dir.path);
        let module_bytes = store
            .get(
                &fake_ref,
                PullPolicy::IfNotPresent,
                &RegistryAuth::Anonymous,
            )
            .await?;
        assert_eq!(3, module_bytes.len());
        assert_eq!(2, module_bytes[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_can_pull_if_policy_always() -> anyhow::Result<()> {
        let fake_client = FakeImageClient::new(vec![("foo/bar:1.0", vec![1, 2, 3], "sha256:123")]);
        let fake_ref = Reference::try_from("foo/bar:1.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client, &scratch_dir.path);
        let module_bytes = store
            .get(&fake_ref, PullPolicy::Always, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(3, module_bytes.len());
        assert_eq!(2, module_bytes[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_does_not_pull_if_policy_never() -> anyhow::Result<()> {
        let fake_client = FakeImageClient::new(vec![("foo/bar:1.0", vec![1, 2, 3], "sha256:123")]);
        let fake_ref = Reference::try_from("foo/bar:1.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client, &scratch_dir.path);
        let module_bytes = store
            .get(&fake_ref, PullPolicy::Never, &RegistryAuth::Anonymous)
            .await;
        assert!(
            module_bytes.is_err(),
            "expected get with pull policy Never to fail but it worked"
        );
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_can_reuse_cached_if_policy_never() -> anyhow::Result<()> {
        let fake_client = FakeImageClient::new(vec![("foo/bar:1.0", vec![1, 2, 3], "sha256:123")]);
        let fake_ref = Reference::try_from("foo/bar:1.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client, &scratch_dir.path);
        let prime_cache = store
            .get(&fake_ref, PullPolicy::Always, &RegistryAuth::Anonymous)
            .await;
        assert!(prime_cache.is_ok());
        let module_bytes = store
            .get(&fake_ref, PullPolicy::Never, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(3, module_bytes.len());
        assert_eq!(2, module_bytes[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_ignores_updates_if_policy_if_not_present() -> anyhow::Result<()> {
        let mut fake_client =
            FakeImageClient::new(vec![("foo/bar:1.0", vec![1, 2, 3], "sha256:123")]);
        let fake_ref = Reference::try_from("foo/bar:1.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client.clone(), &scratch_dir.path);
        let module_bytes_orig = store
            .get(
                &fake_ref,
                PullPolicy::IfNotPresent,
                &RegistryAuth::Anonymous,
            )
            .await?;
        assert_eq!(3, module_bytes_orig.len());
        assert_eq!(2, module_bytes_orig[1]);
        fake_client.update("foo/bar:1.0", vec![4, 5, 6, 7], "sha256:4567");
        let module_bytes_after = store
            .get(
                &fake_ref,
                PullPolicy::IfNotPresent,
                &RegistryAuth::Anonymous,
            )
            .await?;
        assert_eq!(3, module_bytes_after.len());
        assert_eq!(2, module_bytes_after[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_gets_updates_if_policy_always() -> anyhow::Result<()> {
        let mut fake_client =
            FakeImageClient::new(vec![("foo/bar:1.0", vec![1, 2, 3], "sha256:123")]);
        let fake_ref = Reference::try_from("foo/bar:1.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client.clone(), &scratch_dir.path);
        let module_bytes_orig = store
            .get(
                &fake_ref,
                PullPolicy::IfNotPresent,
                &RegistryAuth::Anonymous,
            )
            .await?;
        assert_eq!(3, module_bytes_orig.len());
        assert_eq!(2, module_bytes_orig[1]);
        fake_client.update("foo/bar:1.0", vec![4, 5, 6, 7], "sha256:4567");
        let module_bytes_after = store
            .get(&fake_ref, PullPolicy::Always, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(4, module_bytes_after.len());
        assert_eq!(5, module_bytes_after[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_copes_with_no_tag() -> anyhow::Result<()> {
        let fake_client = FakeImageClient::new(vec![("foo/bar", vec![2, 3], "sha256:23")]);
        let fake_ref = Reference::try_from("foo/bar")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client, &scratch_dir.path);
        let module_bytes = store
            .get(&fake_ref, PullPolicy::Always, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(2, module_bytes.len());
        assert_eq!(3, module_bytes[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_can_pull_if_tag_given_but_policy_omitted() -> anyhow::Result<()> {
        let mut fake_client =
            FakeImageClient::new(vec![("foo/bar:2.0", vec![6, 7, 8], "sha256:678")]);
        let fake_ref = Reference::try_from("foo/bar:2.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client.clone(), &scratch_dir.path);
        let policy = PullPolicy::parse_effective(None, Some(fake_ref.clone()))?;
        let module_bytes_orig = store
            .get(&fake_ref, policy, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(3, module_bytes_orig.len());
        assert_eq!(7, module_bytes_orig[1]);
        fake_client.update("foo/bar:2.0", vec![8, 9], "sha256:89");
        // But with no policy it should *not* re-fetch a tag that's in cache
        let module_bytes_after = store
            .get(&fake_ref, policy, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(3, module_bytes_after.len());
        assert_eq!(7, module_bytes_after[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_always_pulls_if_tag_latest_and_policy_omitted() -> anyhow::Result<()>
    {
        let mut fake_client =
            FakeImageClient::new(vec![("foo/bar:latest", vec![3, 4], "sha256:34")]);
        let fake_ref = Reference::try_from("foo/bar:latest")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client.clone(), &scratch_dir.path);
        let policy = PullPolicy::parse_effective(None, Some(fake_ref.clone()))?;
        let module_bytes_orig = store
            .get(&fake_ref, policy, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(2, module_bytes_orig.len());
        assert_eq!(4, module_bytes_orig[1]);
        fake_client.update("foo/bar:latest", vec![5, 6, 7], "sha256:567");
        let module_bytes_after = store
            .get(&fake_ref, policy, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(3, module_bytes_after.len());
        assert_eq!(6, module_bytes_after[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_always_pulls_if_tag_and_policy_omitted() -> anyhow::Result<()> {
        let mut fake_client = FakeImageClient::new(vec![("foo/bar", vec![3, 4], "sha256:34")]);
        let fake_ref = Reference::try_from("foo/bar")?;
        let scratch_dir = create_temp_dir();
        let store = FileStore::new(fake_client.clone(), &scratch_dir.path);
        let policy = PullPolicy::parse_effective(None, Some(fake_ref.clone()))?;
        let module_bytes_orig = store
            .get(&fake_ref, policy, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(2, module_bytes_orig.len());
        assert_eq!(4, module_bytes_orig[1]);
        fake_client.update("foo/bar", vec![5, 6, 7], "sha256:567");
        let module_bytes_after = store
            .get(&fake_ref, policy, &RegistryAuth::Anonymous)
            .await?;
        assert_eq!(3, module_bytes_after.len());
        assert_eq!(6, module_bytes_after[1]);
        Ok(())
    }
}
