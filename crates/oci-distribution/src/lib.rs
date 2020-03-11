use chrono::prelude::{DateTime, Utc};
use failure::format_err;
use hyperx::header::Header;
use reqwest::header::{HeaderMap, HeaderValue};
use std::convert::TryFrom;
use www_authenticate::{Challenge, ChallengeFields, RawChallenge, WwwAuthenticate};

use crate::errors::*;
pub use crate::manifest::*;
pub use crate::reference::Reference;

const OCI_VERSION_KEY: &str = "Docker-Distribution-Api-Version";

pub mod errors;
pub mod manifest;
pub mod reference;

type OciResult<T> = Result<T, failure::Error>;

/// The OCI client connects to an OCI registry and fetches OCI images.
///
/// An OCI registry is a container registry that adheres to the OCI Distribution
/// specification. DockerHub is one example, as are ACR and GCR. This client
/// provides a native Rust implementation for pulling OCI images.
///
/// Some OCI registries support completely anonymous access. But most require
/// at least an Oauth2 handshake. Typlically, you will want to create a new
/// client, and then run the `auth()` method, which will attempt to get
/// a read-only bearer token. From there, pulling images can be done with
/// the `pull_*` functions.
///
/// For true anonymous access, you can skip `auth()`. This is not recommended
/// unless you are sure that the remote registry does not require Oauth2.
pub struct Client {
    token: Option<RegistryToken>,
}

impl Default for Client {
    fn default() -> Self {
        Client { token: None }
    }
}

impl Client {
    /// According to the v2 specification, 200 and 401 error codes MUST return the
    /// version. It appears that any other response code should be deemed non-v2.
    ///
    /// For this implementation, it will return v2 or an error result. If the error is a
    /// `reqwest` error, the request itself failed. All other error messages mean that
    /// v2 is not supported.
    pub async fn version(&self, host: String) -> OciResult<String> {
        let url = format!("https://{}/v2/", host);
        let res = reqwest::get(&url).await?;
        let disthdr = res.headers().get(OCI_VERSION_KEY);
        let version = disthdr
            .ok_or_else(|| failure::format_err!("no header v2 found"))?
            .to_str()?
            .to_owned();
        Ok(version)
    }

    /// Perform an OAuth v2 auth request if necessary.
    ///
    /// This performs authorization and then stores the token internally to be used
    /// on other requests.
    pub async fn auth(&mut self, image: &Reference, _secret: Option<String>) -> OciResult<()> {
        let cli = reqwest::Client::new();
        // The version request will tell us where to go.
        let url = format!("https://{}/v2/", image.registry());
        let res = cli.get(&url).send().await?;
        let disthdr = res.headers().get(reqwest::header::WWW_AUTHENTICATE);
        if disthdr.is_none() {
            // The Authenticate header can be set to empty string.
            return Ok(());
        }
        let auth = WwwAuthenticate::parse_header(&disthdr.unwrap().as_bytes().into())?;
        let challenge_opt = auth.get::<BearerChallenge>();
        if challenge_opt.is_none() {
            // This means that no challenge was present, even though the header was present.
            // Since we do not handle basic auth, it could be the case that the upstream service
            // is in compatibility mode with a Docker v1 registry.
            return Ok(());
        }

        // Right now, we do read-only auth.
        let pull_perms = format!("repository:{}:pull", image.repository());
        let challenge = challenge_opt.unwrap()[0].clone();
        let realm = challenge.realm.unwrap();
        let service = challenge.service.unwrap();
        let scope = pull_perms.to_owned();

        // TODO: At some point in the future, we should support sending a secret to the
        // server for auth. This particular workflow is for read-only public auth.
        let auth_res = cli
            .get(&realm)
            .query(&[("service", service), ("scope", scope)])
            .send()
            .await?;

        match auth_res.status() {
            reqwest::StatusCode::OK => {
                let docker_token: RegistryToken = auth_res.json().await?;
                self.token = Some(docker_token);
                Ok(())
            }
            _ => {
                let reason = auth_res.text().await?;
                Err(failure::format_err!("failed to authenticate: {}", reason))
            }
        }
    }

    /// Pull a manifest from the remote OCI Distribution service.
    ///
    /// If the connection has already gone through authentication, this will
    /// use the bearer token. Otherwise, this will attempt an anonymous pull.
    pub async fn pull_manifest(&self, image: &Reference) -> OciResult<OciManifest> {
        // We unwrap right now because this try_from literally cannot fail.
        let cli = reqwest::Client::new();
        let url = image.to_v2_manifest_url();
        let request = cli.get(&url);

        let mut headers = HeaderMap::new();
        headers.insert("Accept", "application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.list.v2+json".parse().unwrap());

        if let Some(bearer) = self.token.as_ref() {
            headers.insert("Authorization", bearer.bearer_token().parse().unwrap());
        }

        let res = request.headers(headers).send().await?;

        let status = res.status();
        if res.status().is_client_error() {
            // According to the OCI spec, we should see an error in the message body.
            let err = res.json::<OciEnvelope>().await?;
            // FIXME: This should not have to wrap the error.
            return Err(format_err!("{} on {}", err.errors[0], url));
        } else if status.is_server_error() {
            return Err(format_err!("Server error at {}", url));
        }
        let manifest = res.json::<OciManifest>().await?;
        Ok(manifest)
    }
}

/// A token granted during the OAuth2-like workflow for OCI registries.
#[derive(serde::Deserialize)]
struct RegistryToken {
    access_token: String,
    expires_in: Option<u32>,
    issued_at: Option<DateTime<Utc>>,
}

impl RegistryToken {
    fn bearer_token(&self) -> String {
        format!("Bearer {}", self.access_token)
    }
}

impl Default for RegistryToken {
    fn default() -> Self {
        RegistryToken {
            access_token: "".to_owned(),
            expires_in: None,
            issued_at: None, // We could do Utc::now() if we really need.
        }
    }
}

#[derive(Clone)]
struct BearerChallenge {
    pub realm: Option<String>,
    pub service: Option<String>,
    pub scope: Option<String>,
}

impl Challenge for BearerChallenge {
    fn challenge_name() -> &'static str {
        "Bearer"
    }

    fn from_raw(raw: RawChallenge) -> Option<Self> {
        match raw {
            RawChallenge::Token68(_) => None,
            RawChallenge::Fields(mut map) => Some(BearerChallenge {
                realm: map.remove("realm"),
                scope: map.remove("scope"),
                service: map.remove("service"),
            }),
        }
    }
    fn into_raw(self) -> RawChallenge {
        let mut map = ChallengeFields::new();
        self.realm
            .and_then(|realm| map.insert_static_quoting("realm", realm));
        self.scope
            .and_then(|item| map.insert_static_quoting("scope", item));
        self.service
            .and_then(|item| map.insert_static_quoting("service", item));
        RawChallenge::Fields(map)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::convert::TryFrom;
    #[tokio::test]
    async fn test_version() {
        let c = Client::default();
        let ver = c
            .version("webassembly.azurecr.io".to_owned())
            .await
            .expect("result from version request");
        assert_eq!("registry/2.0".to_owned(), ver);
    }

    #[tokio::test]
    async fn test_auth() {
        let image =
            Reference::try_from("webassembly.azurecr.io/hello-wasm:v1").expect("parsed reference");
        let mut c = Client::default();
        c.auth(&image, None)
            .await
            .expect("result from auth request");

        let tok = c.token.expect("token is available");
        // We test that the token is longer than a minimal hash.
        assert!(tok.access_token.len() > 64);
    }

    #[tokio::test]
    async fn test_pull_manifest() {
        let image =
            Reference::try_from("webassembly.azurecr.io/hello-wasm:v1").expect("parsed reference");
        // Currently, pull_manifest does not perform Authz, so this will fail.
        let c = Client::default();
        c.pull_manifest(&image)
            .await
            .expect_err("pull manifest should fail");

        // But this should pass
        let image =
            Reference::try_from("webassembly.azurecr.io/hello-wasm:v1").expect("parsed reference");
        // Currently, pull_manifest does not perform Authz, so this will fail.
        let mut c = Client::default();
        c.auth(&image, None).await.expect("authenticated");
        let manifest = c
            .pull_manifest(&image)
            .await
            .expect("pull manifest should not fail");

        // The test on the manifest checks all fields. This is just a brief sanity check.
        assert_eq!(manifest.schema_version, 2);
        assert!(!manifest.layers.is_empty());
    }
}
