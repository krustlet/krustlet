#[macro_use]
extern crate serde;

use hyperx::header::Header;
use oauth2::{Client as OauthClient, StandardToken};
use serde::Deserialize;
use std::convert::TryFrom;
use url::Url;
use www_authenticate::{Challenge, ChallengeFields, RawChallenge, WwwAuthenticate};

use crate::errors::*;
use crate::reference::Reference;

const OCI_VERSION_KEY: &str = "Docker-Distribution-Api-Version";

pub mod errors;
pub mod reference;

type OciResult<T> = Result<T, anyhow::Error>;

struct Client {
    token: Option<StandardToken>,
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
            .ok_or_else(|| anyhow::format_err!("no header v2 found"))?
            .to_str()?
            .to_owned();
        Ok(version)
    }

    /// Perform an OAuth v2 auth request if necessary.
    ///
    /// This performs authorization and then stores the token internally to be used
    /// on other requests.
    pub async fn auth(&mut self, host: String, secret: Option<String>) -> OciResult<()> {
        let cli = reqwest::Client::new();
        // The version request will tell us where to go.
        let url = format!("https://{}/v2/", host);
        let res = cli.get(&url).send().await?;
        let disthdr = res.headers().get(reqwest::header::WWW_AUTHENTICATE);
        if disthdr.is_none() {
            // The Authenticate header can be set to empty string.
            return Ok(());
        }
        // FIXME: There must be a more elegant way of passing a header from hyper into WwwAuthenticate.
        let auth = WwwAuthenticate::parse_header(&disthdr.unwrap().to_str()?.to_string().into())?;
        let challenge_opt = auth.get::<BearerChallenge>();
        if challenge_opt.is_none() {
            return Ok(());
        }

        let challenge = challenge_opt.as_ref().unwrap()[0].clone();
        let realm = challenge.realm.unwrap();
        let mut oauth = OauthClient::new(
            "krustlet",
            Url::parse(realm.as_str())?,
            Url::parse(realm.as_str())?,
        );
        oauth.add_scope(challenge.scope.unwrap().as_str());
        oauth.set_client_secret(secret.unwrap_or_else(|| "".to_owned()));

        let token = oauth
            .exchange_client_credentials()
            .with_client(&cli)
            .execute::<StandardToken>()
            .await?;

        self.token = Some(token);

        Ok(())
    }

    pub async fn pull_manifest(&self, image_name: String) -> OciResult<String> {
        // We unwrap right now because this try_from literally cannot fail.
        let reference = Reference::try_from(image_name).unwrap();
        let url = reference.to_v2_manifest_url();
        let res = reqwest::get(&url).await?;
        let status = res.status();
        if res.status().is_client_error() {
            // According to the OCI spec, we should see an error in the message body.
            let err = res.json::<OciEnvelope>().await?;
            // FIXME: This should not have to wrap the error.
            return Err(anyhow::format_err!("{}", err.errors[0]));
        } else if status.is_server_error() {
            return Err(anyhow::format_err!("Server error at {}", url));
        }
        let text = res.text().await?;
        Ok(text)
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
                // NB (mpb): This follows the `remove` pattern from the existing
                // implementations. Not sure why they do this, though.
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
        let mut c = Client::default();
        c.auth("webassembly.azurecr.io".to_owned(), None)
            .await
            .expect("result from version request");
    }

    #[tokio::test]
    async fn test_pull_manifest() {
        // Currently, pull_manifest does not perform Authz, so this will fail.
        let c = Client::default();
        c.pull_manifest("webassembly.azurecr.io/hello:v1".to_owned())
            .await
            .expect_err("pull manifest should fail");
    }
}
