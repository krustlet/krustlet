use futures::StreamExt;
use k8s_openapi::api::core::v1::{ConfigMap, Pod, Secret};
use kube::api::{Api, DeleteParams};

#[derive(Clone, Debug)]
pub enum TestResource {
    Secret(String),
    ConfigMap(String),
    Pod(String),
}

pub struct TestResourceManager {
    namespace: String,
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
    pub fn new(namespace: &str) -> Self {
        TestResourceManager {
            resources: vec![],
            namespace: namespace.to_owned(),
        }
    }

    pub fn push(&mut self, resource: TestResource) {
        self.resources.push(resource)
    }
}

// This needs to be a free function to work nicely with the Drop
// implementation
async fn clean_up_resources(resources: Vec<TestResource>, namespace: String) -> anyhow::Result<()> {
    let cleanup_error_opts: Vec<_> = futures::stream::iter(resources)
        .then(|r| clean_up_resource(r, &namespace))
        .collect()
        .await;
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
