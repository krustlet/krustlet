//! Types for working with registry access secrets

/// A method for authenticating to a registry
pub enum RegistryAuth {
    /// Access the registry anonymously
    Anonymous,
}

pub(crate) trait Authenticable {
    fn apply_authentication(self, auth: &RegistryAuth) -> Self;
}

impl Authenticable for reqwest::RequestBuilder {
    fn apply_authentication(self, auth: &RegistryAuth) -> Self {
        match auth {
            RegistryAuth::Anonymous => self,
        }
    }
}
