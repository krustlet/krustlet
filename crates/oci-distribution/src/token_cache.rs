use crate::reference::Reference;
use serde::Deserialize;
use std::collections::BTreeMap;

/// A token granted during the OAuth2-like workflow for OCI registries.
#[derive(Deserialize)]
#[serde(untagged)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RegistryToken {
    Token { token: String },
    AccessToken { access_token: String },
}

pub(crate) enum RegistryTokenType {
    Bearer(RegistryToken),
    Basic(String, String),
}

impl RegistryToken {
    pub fn bearer_token(&self) -> String {
        format!("Bearer {}", self.token())
    }

    pub fn token(&self) -> &str {
        match self {
            RegistryToken::Token { token } => token,
            RegistryToken::AccessToken { access_token } => access_token,
        }
    }
}

/// Desired operation for registry authentication
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryOperation {
    /// Authenticate for push operations
    Push,
    /// Authenticate for pull operations
    Pull,
}

#[derive(Default)]
pub(crate) struct TokenCache {
    // (registry, repository, scope) -> (token, expiration)
    tokens: BTreeMap<(String, String, RegistryOperation), (RegistryTokenType, usize)>,
}

impl TokenCache {
    pub(crate) fn new() -> Self {
        TokenCache {
            tokens: BTreeMap::new(),
        }
    }

    pub(crate) fn insert(
        &mut self,
        reference: &Reference,
        op: RegistryOperation,
        token: RegistryTokenType,
    ) {
        todo!()
    }

    pub(crate) fn get(
        &self,
        reference: &Reference,
        op: RegistryOperation,
    ) -> Option<RegistryTokenType> {
        todo!()
    }

    pub(crate) fn contains_key(&self, reference: &Reference, op: RegistryOperation) -> bool {
        todo!()
    }
}
