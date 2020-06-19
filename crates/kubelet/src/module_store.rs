//! Stores of container module images
use crate::image_client::ImageClient;
use crate::pod::Pod;

use async_trait::async_trait;
use log::debug;
use oci_distribution::Reference;
use tokio::sync::Mutex;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Specifies how the store should check for module updates
#[derive(PartialEq, Debug)]
pub enum ModulePullPolicy {
    /// Always return the module as it currently appears in the
    /// upstream registry
    Always,
    /// Return the module as it is currently cached in the local store if
    /// present; fetch it from the upstream registry only if it it not
    /// present in the local store
    IfNotPresent,
    /// Never fetch the module from the upstream registry; if it is not
    /// available locally then return an error
    Never,
}

impl ModulePullPolicy {
    /// Parses a module pull policy from a Kubernetes ImagePullPolicy string
    pub fn parse(name: Option<String>) -> anyhow::Result<Option<ModulePullPolicy>> {
        match name {
            None => Ok(None),
            Some(n) => ModulePullPolicy::parse_str(&n[..]),
        }
    }

    fn parse_str(name: &str) -> anyhow::Result<Option<ModulePullPolicy>> {
        match name {
            "Always" => Ok(Some(Self::Always)),
            "IfNotPresent" => Ok(Some(Self::IfNotPresent)),
            "Never" => Ok(Some(Self::Never)),
            other => Err(anyhow::anyhow!("unrecognized pull policy {}", other)),
        }
    }
}

/// A store of container modules.
///
/// This provides the ability to get a module's bytes given an image [`Reference`].
///
/// # Example
///  ```rust
/// use async_trait::async_trait;
/// use oci_distribution::Reference;
/// use kubelet::module_store::ModuleStore;
/// use std::collections::HashMap;
///
/// struct InMemoryStore {
///     modules: HashMap<Reference, Vec<u8>>,
/// };
///
/// #[async_trait]
/// impl ModuleStore for InMemoryStore {
///     async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
///         match self.modules.get(image_ref) {
///             Some(bytes) => Ok(bytes.clone()),
///             None => todo!("Fetch the bytes from some sort of remore store (e.g., OCI Distribution)")
///         }
///     }
///
///     async fn is_present(&self, image_ref: &Reference) -> bool {
///         self.modules.get(image_ref).is_some()
///     }
///
///     async fn is_present_with_digest(&self, image_ref: &Reference, digest: String) -> bool {
///         false
///     }
///
///     async fn pull(&self, image_ref: &Reference) -> anyhow::Result<()> {
///         Err(anyhow::anyhow!("InMemoryStore does not support registry pull"))
///     }
///
///     async fn resolve_registry_digest(&self, image_ref: &Reference) -> anyhow::Result<String> {
///         Err(anyhow::anyhow!("InMemoryStore does not support registry pull"))
///     }
/// }
/// ```
#[async_trait]
pub trait ModuleStore {
    /// Get a module's data given its image `Reference`.
    ///
    /// It is up to the implementation to establish caching policies.
    /// However, the implementation must fail if the image is not present
    /// locally.
    async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>>;

    /// Whether the specified module is already present in the store.
    async fn is_present(&self, image_ref: &Reference) -> bool;

    /// Whether the specified module is already present in the store with the specified digest.
    async fn is_present_with_digest(&self, image_ref: &Reference, digest: String) -> bool;

    /// Pull a module from a remote source into the store.
    async fn pull(&self, image_ref: &Reference) -> anyhow::Result<()>;

    /// Get the digest of the specified module in its source registry.
    async fn resolve_registry_digest(&self, image_ref: &Reference) -> anyhow::Result<String>;

    /// Get a module's data given its image `Reference`
    async fn get(
        &self,
        image_ref: &Reference,
        pull_policy: Option<ModulePullPolicy>,
    ) -> anyhow::Result<Vec<u8>> {
        // Specification from https://kubernetes.io/docs/concepts/configuration/overview/#container-images):
        let effective_pull_policy = pull_policy.unwrap_or(match image_ref.tag() {
            Some("latest") | None => ModulePullPolicy::Always,
            _ => ModulePullPolicy::IfNotPresent,
        });

        match effective_pull_policy {
            ModulePullPolicy::IfNotPresent => {
                if !self.is_present(image_ref).await {
                    self.pull(image_ref).await?
                }
            }
            ModulePullPolicy::Always => {
                if !self
                    .is_present_with_digest(
                        image_ref,
                        self.resolve_registry_digest(image_ref).await?,
                    )
                    .await
                {
                    self.pull(image_ref).await?
                }
            }
            ModulePullPolicy::Never => (),
        };

        self.get_local(image_ref).await
    }

    /// Fetch all container modules for a given `Pod` storing the name of the
    /// container and the module's data as key/value pairs in a hashmap.
    ///
    /// This will fetch all of the container modules in parallel.
    ///
    /// # Panics
    ///
    /// This panics if any of the pod's containers do not have an image associated with them
    async fn fetch_pod_modules(&self, pod: &Pod) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        debug!(
            "Fetching all the container modules for pod '{}'",
            pod.name()
        );
        // Fetch all of the container modules in parallel
        let container_module_futures = pod.containers().iter().map(move |container| {
            let image = container
                .image
                .clone()
                .expect("FATAL ERROR: container must have an image");
            let reference = Reference::try_from(image).unwrap();
            let pull_policy = ModulePullPolicy::parse(container.image_pull_policy.clone()).unwrap();
            async move {
                Ok((
                    container.name.clone(),
                    self.get(&reference, pull_policy).await?,
                ))
            }
        });

        // Collect the container modules into a HashMap for quick lookup
        futures::future::join_all(container_module_futures)
            .await
            .into_iter()
            .collect()
    }
}

/// A module store that keeps modules cached on the file system
///
/// This type is generic over the type of Kubernetes client used
/// to fetch modules from a remote store. This client is expected
/// to be an [`ImageClient`]
pub struct FileModuleStore<C> {
    root_dir: PathBuf,
    client: Arc<Mutex<C>>,
}

impl<C> FileModuleStore<C> {
    /// Create a new `FileModuleStore`
    pub fn new<T: AsRef<Path>>(client: C, root_dir: T) -> Self {
        Self {
            root_dir: root_dir.as_ref().into(),
            client: Arc::new(Mutex::new(client)),
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

    async fn store(
        &self,
        image_ref: &Reference,
        digest: Option<String>,
        contents: &[u8],
    ) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(self.pull_path(image_ref)).await?;
        let digest_path = self.digest_file_path(image_ref);
        if digest_path.exists() {
            tokio::fs::remove_file(&digest_path).await?;
        }
        let module_path = self.pull_file_path(image_ref);
        tokio::fs::write(&module_path, contents).await?;
        if let Some(d) = digest {
            tokio::fs::write(&digest_path, d).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl<C: ImageClient + Send> ModuleStore for FileModuleStore<C> {
    async fn get_local(&self, image_ref: &Reference) -> anyhow::Result<Vec<u8>> {
        let path = self.pull_file_path(image_ref);
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Image ref {} not available locally",
                image_ref
            ));
        }

        debug!("Fetching image ref '{:?}' from disk", image_ref);
        Ok(tokio::fs::read(path).await?)
    }

    async fn is_present(&self, image_ref: &Reference) -> bool {
        let path = self.pull_file_path(image_ref);
        path.exists()
    }

    async fn is_present_with_digest(&self, image_ref: &Reference, digest: String) -> bool {
        let path = self.digest_file_path(image_ref);
        path.exists() && file_content_is(path, digest).await
    }

    async fn pull(&self, image_ref: &Reference) -> anyhow::Result<()> {
        debug!("Pulling image ref '{:?}' from registry", image_ref);
        let (contents, digest) = self.client.lock().await.pull_with_digest(image_ref).await?;
        self.store(image_ref, digest, &contents).await?;
        Ok(())
    }

    async fn resolve_registry_digest(&self, image_ref: &Reference) -> anyhow::Result<String> {
        self.client.lock().await.fetch_digest(image_ref).await
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

impl<C> Clone for FileModuleStore<C> {
    fn clone(&self) -> Self {
        Self {
            root_dir: self.root_dir.clone(),
            client: self.client.clone(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::RwLock;

    #[tokio::test]
    async fn can_parse_pull_policies() {
        assert_eq!(None, ModulePullPolicy::parse(None).unwrap());
        assert_eq!(
            ModulePullPolicy::Always,
            ModulePullPolicy::parse(Some("Always".to_owned()))
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            ModulePullPolicy::IfNotPresent,
            ModulePullPolicy::parse(Some("IfNotPresent".to_owned()))
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            ModulePullPolicy::Never,
            ModulePullPolicy::parse(Some("Never".to_owned()))
                .unwrap()
                .unwrap()
        );
        assert!(
            ModulePullPolicy::parse(Some("IfMoonMadeOfGreenCheese".to_owned())).is_err(),
            "Expected parse failure but didn't get one"
        );
    }

    #[derive(Clone)]
    struct FakeImageClient {
        images: Arc<RwLock<HashMap<String, (Vec<u8>, Option<String>)>>>,
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
                images.insert(name.to_owned(), (content, Some(digest.to_owned())));
            }
            client
        }

        fn update(&mut self, key: &str, content: Vec<u8>, digest: &str) -> () {
            let mut images = self
                .images
                .write()
                .expect("should be able to write to images");
            images.insert(key.to_owned(), (content, Some(digest.to_owned())));
        }
    }
    #[async_trait]
    impl ImageClient for FakeImageClient {
        async fn pull_with_digest(
            &mut self,
            image_ref: &Reference,
        ) -> anyhow::Result<(Vec<u8>, Option<String>)> {
            let images = self
                .images
                .read()
                .expect("should be able to read from images");
            match images.get(image_ref.whole()) {
                Some(v) => Ok(v.clone()),
                None => Err(anyhow::anyhow!("error pulling module")),
            }
        }

        async fn fetch_digest(&mut self, _image_ref: &Reference) -> anyhow::Result<String> {
            Ok("sha123:456".to_owned())
        }
    }

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) -> () {
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
        let store = FileModuleStore::new(fake_client, &scratch_dir.path);
        let module_bytes = store
            .get(&fake_ref, Some(ModulePullPolicy::IfNotPresent))
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
        let store = FileModuleStore::new(fake_client, &scratch_dir.path);
        let module_bytes = store.get(&fake_ref, Some(ModulePullPolicy::Always)).await?;
        assert_eq!(3, module_bytes.len());
        assert_eq!(2, module_bytes[1]);
        Ok(())
    }

    #[tokio::test]
    async fn file_module_store_does_not_pull_if_policy_never() -> anyhow::Result<()> {
        let fake_client = FakeImageClient::new(vec![("foo/bar:1.0", vec![1, 2, 3], "sha256:123")]);
        let fake_ref = Reference::try_from("foo/bar:1.0")?;
        let scratch_dir = create_temp_dir();
        let store = FileModuleStore::new(fake_client, &scratch_dir.path);
        let module_bytes = store.get(&fake_ref, Some(ModulePullPolicy::Never)).await;
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
        let store = FileModuleStore::new(fake_client, &scratch_dir.path);
        let prime_cache = store.get(&fake_ref, Some(ModulePullPolicy::Always)).await;
        assert!(prime_cache.is_ok());
        let module_bytes = store.get(&fake_ref, Some(ModulePullPolicy::Never)).await?;
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
        let store = FileModuleStore::new(fake_client.clone(), &scratch_dir.path);
        let module_bytes_orig = store
            .get(&fake_ref, Some(ModulePullPolicy::IfNotPresent))
            .await?;
        assert_eq!(3, module_bytes_orig.len());
        assert_eq!(2, module_bytes_orig[1]);
        fake_client.update("foo/bar:1.0", vec![4, 5, 6, 7], "sha256:4567");
        let module_bytes_after = store
            .get(&fake_ref, Some(ModulePullPolicy::IfNotPresent))
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
        let store = FileModuleStore::new(fake_client.clone(), &scratch_dir.path);
        let module_bytes_orig = store
            .get(&fake_ref, Some(ModulePullPolicy::IfNotPresent))
            .await?;
        assert_eq!(3, module_bytes_orig.len());
        assert_eq!(2, module_bytes_orig[1]);
        fake_client.update("foo/bar:1.0", vec![4, 5, 6, 7], "sha256:4567");
        let module_bytes_after = store.get(&fake_ref, Some(ModulePullPolicy::Always)).await?;
        assert_eq!(4, module_bytes_after.len());
        assert_eq!(5, module_bytes_after[1]);
        Ok(())
    }
}
