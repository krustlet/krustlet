use futures::StreamExt;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, Secret};
#[cfg(target_os = "linux")]
use k8s_openapi::api::{core::v1::PersistentVolumeClaim, storage::v1::StorageClass};
use kube::api::{Api, DeleteParams, PostParams};
use serde_json::json;

#[derive(Clone, Debug)]
pub enum TestResource {
    Secret(String),
    ConfigMap(String),
    Pod(String),
    #[cfg(target_os = "linux")]
    StorageClass(String),
    #[cfg(target_os = "linux")]
    Pvc(String),
}

#[derive(Clone, Debug)]
pub enum TestResourceSpec {
    Secret(String, Vec<(String, String)>),
    ConfigMap(String, Vec<(String, String)>),
    #[cfg(target_os = "linux")]
    StorageClass(String, String), // resource name, provisioner
    #[cfg(target_os = "linux")]
    Pvc(String, String), // name, storage class
}

impl TestResourceSpec {
    pub fn secret(resource_name: &str, value_name: &str, value: &str) -> Self {
        Self::Secret(
            resource_name.to_owned(),
            vec![(value_name.to_owned(), value.to_owned())],
        )
    }

    pub fn secret_multi(resource_name: &str, entries: &[(&str, &str)]) -> Self {
        Self::Secret(
            resource_name.to_owned(),
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    pub fn config_map(resource_name: &str, value_name: &str, value: &str) -> Self {
        Self::ConfigMap(
            resource_name.to_owned(),
            vec![(value_name.to_owned(), value.to_owned())],
        )
    }

    pub fn config_map_multi(resource_name: &str, entries: &[(&str, &str)]) -> Self {
        Self::ConfigMap(
            resource_name.to_owned(),
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

pub struct TestResourceManager {
    namespace: String,
    client: kube::Client,
    resources: Vec<TestResource>,
}

impl Drop for TestResourceManager {
    fn drop(&mut self) {
        let resources = self.resources.clone();
        let namespace = self.namespace.clone();
        let t = std::thread::spawn(move || {
            let rt =
                tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime for cleanup");
            rt.block_on(clean_up_resources(resources, namespace))
        });

        let thread_result = t.join();
        let cleanup_result = thread_result.expect("Failed to clean up WASI test resources");
        cleanup_result.unwrap()
    }
}

impl TestResourceManager {
    pub async fn initialise(namespace: &str, client: kube::Client) -> anyhow::Result<Self> {
        let namespaces: Api<Namespace> = Api::all(client.clone());
        namespaces
            .create(
                &PostParams::default(),
                &serde_json::from_value(json!({
                        "apiVersion": "v1",
                        "kind": "Namespace",
                        "metadata": {
                            "name": namespace
                        },
                        "spec": {}
                }))?,
            )
            .await?;

        // k8s seems to need a bit of time for namespace permissions to flow
        // through the system.  TODO: make this less worse
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        let image_pull_secret_opt = std::env::var("KRUSTLET_E2E_IMAGE_PULL_SECRET");

        if let Ok(image_pull_secret) = image_pull_secret_opt {
            let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
            secrets
                .create(
                    &PostParams::default(),
                    &serde_json::from_value(json!({
                            "apiVersion": "v1",
                            "kind": "Secret",
                            "metadata": {
                                "name": "registry-creds",
                            },
                            "type": "kubernetes.io/dockerconfigjson",
                            "data": {
                                ".dockerconfigjson": image_pull_secret,
                            },
                    }))?,
                )
                .await?;
        };

        Ok(TestResourceManager {
            namespace: namespace.to_owned(),
            client: client.clone(),
            resources: vec![],
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn push(&mut self, resource: TestResource) {
        self.resources.push(resource)
    }

    pub async fn set_up_resources(
        &mut self,
        resources: Vec<TestResourceSpec>,
    ) -> anyhow::Result<()> {
        for resource in resources {
            self.set_up_resource(&resource).await?;
        }

        Ok(())
    }

    async fn set_up_resource(&mut self, resource: &TestResourceSpec) -> anyhow::Result<()> {
        match resource {
            TestResourceSpec::Secret(resource_name, entries) => {
                Api::<Secret>::namespaced(self.client.clone(), self.namespace())
                    .create(
                        &PostParams::default(),
                        &serde_json::from_value(json!({
                            "apiVersion": "v1",
                            "kind": "Secret",
                            "metadata": {
                                "name": resource_name
                            },
                            "stringData": object_of_tuples(entries)
                        }))?,
                    )
                    .await?;
                self.push(TestResource::Secret(resource_name.to_owned()));
            }
            TestResourceSpec::ConfigMap(resource_name, entries) => {
                Api::<ConfigMap>::namespaced(self.client.clone(), self.namespace())
                    .create(
                        &PostParams::default(),
                        &serde_json::from_value(json!({
                            "apiVersion": "v1",
                            "kind": "ConfigMap",
                            "metadata": {
                                "name": resource_name
                            },
                            "data": object_of_tuples(entries)
                        }))?,
                    )
                    .await?;
                self.push(TestResource::ConfigMap(resource_name.to_owned()));
            }
            #[cfg(target_os = "linux")]
            TestResourceSpec::StorageClass(resource_name, provisioner) => {
                Api::<StorageClass>::all(self.client.clone())
                    .create(
                        &PostParams::default(),
                        &serde_json::from_value(json!({
                            "apiVersion": "storage.k8s.io/v1",
                            "kind": "StorageClass",
                            "metadata": {
                                "name": resource_name
                            },
                            "provisioner": provisioner,
                            "reclaimPolicy": "Delete",
                            "volumeBindingMode": "Immediate",
                            "allowVolumeExpansion": true
                        }))?,
                    )
                    .await?;
                self.push(TestResource::StorageClass(resource_name.to_owned()));
            }
            #[cfg(target_os = "linux")]
            TestResourceSpec::Pvc(resource_name, storage_class) => {
                Api::<PersistentVolumeClaim>::namespaced(self.client.clone(), self.namespace())
                    .create(
                        &PostParams::default(),
                        &serde_json::from_value(json!({
                            "apiVersion": "v1",
                            "kind": "PersistentVolumeClaim",
                            "metadata": {
                                "name": resource_name
                            },
                            "spec": {
                                "accessModes": [
                                    "ReadWriteOnce",
                                ],
                                "resources": {
                                    "requests": {
                                        "storage": "1Gi",
                                    }
                                },
                                "storageClassName": storage_class
                            }
                        }))?,
                    )
                    .await?;
                self.push(TestResource::Pvc(resource_name.to_owned()));
            }
        }

        Ok(())
    }
}

// This needs to be a free function to work nicely with the Drop
// implementation
async fn clean_up_resources(resources: Vec<TestResource>, namespace: String) -> anyhow::Result<()> {
    // Reverse the order of cleanup. Often times resources that are dependent on others (like PVC on
    // a storage class) should be deleted first. Since they would have been created first and pushed
    // onto the Vec in that order, the naive/simple way to do this is to just reverse, which should
    // work for tests
    let mut cleanup_error_opts: Vec<_> = futures::stream::iter(resources.into_iter().rev())
        .then(|r| clean_up_resource(r, &namespace))
        .collect()
        .await;
    cleanup_error_opts.push(clean_up_namespace(&namespace).await);

    let cleanup_errors: Vec<_> = cleanup_error_opts
        .iter()
        .filter(|e| e.is_some())
        .map(|e| e.as_ref().unwrap().to_string())
        .filter(|s| !s.contains(r#"reason: "NotFound""#))
        .collect();

    if cleanup_errors.is_empty() {
        Ok(())
    } else {
        let cleanup_failure_text = format!(
            "Error(s) cleaning up resources: {}",
            cleanup_errors.join(", ")
        );
        Err(anyhow::anyhow!(cleanup_failure_text))
    }
}

async fn clean_up_resource(resource: TestResource, namespace: &str) -> Option<String> {
    let client = kube::Client::try_default()
        .await
        .expect("Failed to create client");

    match resource {
        TestResource::Secret(name) => Api::<Secret>::namespaced(client.clone(), namespace)
            .delete(&name, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("secret {} ({})", name, e)),
        TestResource::ConfigMap(name) => Api::<ConfigMap>::namespaced(client.clone(), namespace)
            .delete(&name, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("configmap {} ({})", name, e)),
        TestResource::Pod(name) => Api::<Pod>::namespaced(client.clone(), namespace)
            .delete(&name, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("pod {} ({})", name, e)),
        #[cfg(target_os = "linux")]
        TestResource::StorageClass(name) => Api::<StorageClass>::all(client.clone())
            .delete(&name, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("storage class {} ({})", name, e)),
        #[cfg(target_os = "linux")]
        TestResource::Pvc(name) => {
            Api::<PersistentVolumeClaim>::namespaced(client.clone(), namespace)
                .delete(&name, &DeleteParams::default())
                .await
                .err()
                .map(|e| format!("PVC {} ({})", name, e))
        }
    }
}

async fn clean_up_namespace(namespace: &str) -> Option<String> {
    let client = kube::Client::try_default()
        .await
        .expect("Failed to create client");

    let namespaces: Api<Namespace> = Api::all(client.clone());

    namespaces
        .delete(namespace, &DeleteParams::default())
        .await
        .err()
        .map(|e| format!("namespace {} ({})", namespace, e))
}

fn object_of_tuples(source: &[(String, String)]) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    for (key, value) in source {
        map.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    serde_json::Value::Object(map)
}
