use futures::StreamExt;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, Secret};
use kube::api::{Api, DeleteParams, PostParams};
use serde_json::json;

#[derive(Clone, Debug)]
pub enum TestResource {
    Secret(String),
    ConfigMap(String),
    Pod(String),
}

#[derive(Clone, Debug)]
pub enum TestResourceSpec {
    Secret(String, String, String),    // single value per secret for now
    ConfigMap(String, String, String), // single value per cm for now
}

impl TestResourceSpec {
    pub fn secret(resource_name: &str, value_name: &str, value: &str) -> Self {
        Self::Secret(
            resource_name.to_owned(),
            value_name.to_owned(),
            value.to_owned(),
        )
    }

    pub fn config_map(resource_name: &str, value_name: &str, value: &str) -> Self {
        Self::ConfigMap(
            resource_name.to_owned(),
            value_name.to_owned(),
            value.to_owned(),
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
            let mut rt =
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
        tokio::time::delay_for(tokio::time::Duration::from_millis(100)).await;

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
        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), self.namespace());
        let config_maps: Api<ConfigMap> = Api::namespaced(self.client.clone(), self.namespace());

        match resource {
            TestResourceSpec::Secret(resource_name, value_name, value) => {
                secrets
                    .create(
                        &PostParams::default(),
                        &serde_json::from_value(json!({
                            "apiVersion": "v1",
                            "kind": "Secret",
                            "metadata": {
                                "name": resource_name
                            },
                            "stringData": {
                                value_name: value
                            }
                        }))?,
                    )
                    .await?;
                self.push(TestResource::Secret(resource_name.to_owned()));
            }
            TestResourceSpec::ConfigMap(resource_name, value_name, value) => {
                config_maps
                    .create(
                        &PostParams::default(),
                        &serde_json::from_value(json!({
                            "apiVersion": "v1",
                            "kind": "ConfigMap",
                            "metadata": {
                                "name": resource_name
                            },
                            "data": {
                                value_name: value
                            }
                        }))?,
                    )
                    .await?;
                self.push(TestResource::ConfigMap(resource_name.to_owned()));
            }
        }

        Ok(())
    }
}

// This needs to be a free function to work nicely with the Drop
// implementation
async fn clean_up_resources(resources: Vec<TestResource>, namespace: String) -> anyhow::Result<()> {
    let mut cleanup_error_opts: Vec<_> = futures::stream::iter(resources)
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

async fn clean_up_resource(resource: TestResource, namespace: &String) -> Option<String> {
    let client = kube::Client::try_default()
        .await
        .expect("Failed to create client");

    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

    match resource {
        TestResource::Secret(name) => secrets
            .delete(&name, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("secret {} ({})", name, e)),
        TestResource::ConfigMap(name) => config_maps
            .delete(&name, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("configmap {} ({})", name, e)),
        TestResource::Pod(name) => pods
            .delete(&name, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("pod {} ({})", name, e)),
    }
}

async fn clean_up_namespace(namespace: &String) -> Option<String> {
    let client = kube::Client::try_default()
        .await
        .expect("Failed to create client");

    let namespaces: Api<Namespace> = Api::all(client.clone());

    namespaces
        .delete(&namespace, &DeleteParams::default())
        .await
        .err()
        .map(|e| format!("namespace {} ({})", namespace, e))
}
