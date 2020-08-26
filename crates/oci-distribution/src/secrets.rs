//! Types for working with registry access secrets

/// Contains secrets for accessing a set of registries
pub struct RegistrySecrets {}

/// Contains a secret for accessing a registry
pub struct RegistrySecret {}

impl RegistrySecrets {
    /// A `RegistrySecrets` that contains no secrets
    pub fn none() -> Self {
        RegistrySecrets {}
    }

    /// Gets the secret for the specified registry, if present
    pub fn find_registry_secret(&self, _registry: &str) -> Option<RegistrySecret> {
        None
    }
}
