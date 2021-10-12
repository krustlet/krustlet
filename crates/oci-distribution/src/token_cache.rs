use crate::reference::Reference;
use serde::Deserialize;
use std::collections::BTreeMap;
use tracing::{debug, warn};

/// A token granted during the OAuth2-like workflow for OCI registries.
#[derive(Deserialize, Clone)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryOperation {
    /// Authenticate for push operations
    Push,
    /// Authenticate for pull operations
    Pull,
}

#[derive(Default)]
pub(crate) struct TokenCache {
    // (registry, repository, scope) -> (token, expiration)
    tokens: BTreeMap<(String, String, RegistryOperation), (RegistryTokenType, u64)>,
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
        let expiration = match token {
            RegistryTokenType::Basic(_, _) => u64::MAX,
            RegistryTokenType::Bearer(ref t) => {
                let token_str = t.token();
                match jwt::Token::<
                        jwt::header::Header,
                        jwt::claims::Claims,
                        jwt::token::Unverified,
                    >::parse_unverified(token_str)
                    {
                        Ok(token) => token.claims().registered.expiration.unwrap_or(u64::MAX),
                        Err(error) => {
                            warn!(?error, "Invalid bearer token");
                            return;
                        }
                    }
            }
        };
        let registry = reference.resolve_registry().to_string();
        let repository = reference.repository().to_string();
        debug!(%registry, %repository, ?op, %expiration, "Inserting token");
        self.tokens
            .insert((registry, repository, op), (token, expiration));
    }

    pub(crate) fn get(
        &self,
        reference: &Reference,
        op: RegistryOperation,
    ) -> Option<&RegistryTokenType> {
        let registry = reference.resolve_registry().to_string();
        let repository = reference.repository().to_string();
        match self.tokens.get(&(registry.clone(), repository.clone(), op)) {
            Some((ref token, expiration)) => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let now = SystemTime::now();
                let epoch = now
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();
                if epoch > *expiration {
                    debug!(%registry, %repository, ?op, %expiration, miss=false, expired=true, "Fetching token");
                    None
                } else {
                    debug!(%registry, %repository, ?op, %expiration, miss=false, expired=false, "Fetching token");
                    Some(token)
                }
            }
            None => {
                debug!(%registry, %repository, ?op, miss=true, "Fetching token");
                None
            }
        }
    }

    pub(crate) fn contains_key(&self, reference: &Reference, op: RegistryOperation) -> bool {
        self.get(reference, op).is_some()
    }
}
