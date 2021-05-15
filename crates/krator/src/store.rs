use std::collections::HashMap;
use std::sync::Arc;

use kube::api::DynamicObject;
use kube::api::GroupVersionKind;
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

use crate::object::ObjectKey;

type ResourceMap = HashMap<GroupVersionKind, HashMap<ObjectKey, serde_json::Value>>;

/// Defines Store type for caching Kubernetes objects locally.
///
/// * State is held in `Arc` so it is cheap to clone.
/// * Collections are scoped by {group, version, kind, namespace, name}.
/// * Objects are stored as [DynamicObject](kube::api::DynamicObject)s.
///
/// ```
/// # use krator::Store;
/// # use k8s_openapi::api::core::v1::Pod;
/// #
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// # let store = Store::new();
///
/// let pod = store.get::<Pod>(Some("namespace"), "name").await?;
///
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Store {
    objects: Arc<RwLock<ResourceMap>>,
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

impl Store {
    /// Initialize empty store.
    pub fn new() -> Self {
        Store {
            objects: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Clear cache for specified object kind.
    pub(crate) async fn reset(&self, gvk: &GroupVersionKind) {
        let mut objects = self.objects.write().await;
        let key = gvk.clone();
        let resource_objects = (*objects).entry(key).or_insert_with(HashMap::new);
        resource_objects.clear();
    }

    /// Delete a cached object.
    pub(crate) async fn delete_gvk(
        &self,
        namespace: Option<String>,
        name: String,
        gvk: &GroupVersionKind,
    ) {
        let mut objects = self.objects.write().await;
        let key = gvk.clone();
        let resource_objects = (*objects).entry(key).or_insert_with(HashMap::new);
        let object_key = ObjectKey::new(namespace, name);
        resource_objects.remove(&object_key);
    }

    /// Insert an object that has already been type erased.
    pub(crate) async fn insert_gvk(
        &self,
        namespace: Option<String>,
        name: String,
        gvk: &GroupVersionKind,
        dynamic_object: DynamicObject,
    ) {
        let mut objects = self.objects.write().await;
        let key = gvk.clone();
        let resource_objects = (*objects).entry(key).or_insert_with(HashMap::new);
        let object_key = ObjectKey::new(namespace, name);
        resource_objects.insert(object_key, serde_json::to_value(&dynamic_object).unwrap());
    }

    /// Fetch an object.
    ///
    /// # Errors
    ///
    /// * If the serialized data cannot be deserialized as type `R`.
    ///
    /// # Returns
    ///
    /// This method will return `None` if:
    ///
    /// * The resource `GroupVersionKind` is not being tracked by any watcher.
    /// * Within the cache for the specific resource, the (namespace, name) key
    ///   is not found.
    // TODO: Multi-watcher cache conflict #603
    pub async fn get<R: 'static + k8s_openapi::Resource + Clone + DeserializeOwned>(
        &self,
        namespace: Option<&str>,
        name: &str,
    ) -> anyhow::Result<Option<R>> {
        let objects = self.objects.read().await;
        let key = GroupVersionKind::gvk(R::GROUP, R::VERSION, R::KIND).unwrap();
        let object_key = ObjectKey::new(namespace.map(|s| s.to_string()), name.to_string());
        match (*objects).get(&key) {
            Some(resource_objects) => match resource_objects.get(&object_key) {
                Some(value) => match serde_json::from_value::<R>(value.clone()) {
                    Ok(object) => Ok(Some(object)),
                    Err(e) => {
                        anyhow::bail!(
                            "Could not interpret interred object as type {}/{} {}: {:?}",
                            R::GROUP,
                            R::VERSION,
                            R::KIND,
                            e
                        );
                    }
                },
                None => Ok(None),
            },
            // TODO: Should this be an error since we probably arent tracking that resource type?
            None => Ok(None),
        }
    }
}
