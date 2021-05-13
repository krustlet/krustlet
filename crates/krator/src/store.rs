use std::collections::HashMap;

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
/// let store = Store::new();
///
/// let pod = Pod::default();
///
/// store.insert(Some("namespace"), "name", pod).await;
///
/// // Elsewhere
/// let pod = store.get::<Pod>(Some("namespace"), "name").await?;
///
/// # Ok(())
/// # }
/// ```
pub struct Store {
    objects: RwLock<ResourceMap>,
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
            objects: RwLock::new(HashMap::new()),
        }
    }

    /// Insert a new object.
    pub async fn insert<R: 'static + k8s_openapi::Resource + Sync + Send + serde::Serialize>(
        &self,
        namespace: Option<&str>,
        name: &str,
        object: R,
    ) {
        let mut objects = self.objects.write().await;
        let key = GroupVersionKind::gvk(R::GROUP, R::VERSION, R::KIND).unwrap();
        let resource_objects = (*objects).entry(key).or_insert_with(HashMap::new);
        let object_key = ObjectKey::new(namespace.map(|s| s.to_string()), name.to_string());
        resource_objects.insert(object_key, serde_json::to_value(&object).unwrap());
    }

    /// Clear cache for specified object kind.
    pub async fn reset(&self, gvk: &GroupVersionKind) {
        let mut objects = self.objects.write().await;
        let key = gvk.clone();
        let resource_objects = (*objects).entry(key).or_insert_with(HashMap::new);
        resource_objects.clear();
    }

    /// Delete a cached object.
    pub async fn delete_any(
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
    pub async fn insert_any(
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
