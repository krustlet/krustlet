//! Resolves image pull secrets

use oci_distribution::secrets::RegistryAuth;

/// Resolves registry authentication from image pull secrets
pub struct RegistryAuthResolver {
    kube_client: kube::Client,
    pod_namespace: String,
}

impl RegistryAuthResolver {
    /// Creates a resolver for the given pod
    pub fn new(client: &kube::Client, pod: &crate::pod::Pod) -> Self {
        RegistryAuthResolver {
            kube_client: client.clone(),
            pod_namespace: pod.namespace().to_owned(),
        }
    }
    /// Get the registry authentication method appropriate to the given image reference
    pub async fn resolve_registry_auth(&self, reference: &oci_distribution::Reference) -> anyhow::Result<RegistryAuth> {
        Ok(RegistryAuth::Anonymous)
    }
}
