#[macro_use]
extern crate serde;

use serde::Deserialize;
use std::convert::TryFrom;

use crate::errors::*;
use crate::reference::Reference;

/// The OCI Distribution specification version 2.
///
/// This marker is used in URLs for accessing OCI Distributiion endpoints.
const OCI_VERSION_2: &str = "v2";
const OCI_VERSION_KEY: &str = "Docker-Distribution-Api-Version";

pub mod errors;
pub mod reference;

type OciResult<T> = Result<T, anyhow::Error>;

struct Client {}

impl Default for Client {
    fn default() -> Self {
        Client {}
    }
}

impl Client {
    pub async fn version(&self, host: String) -> OciResult<String> {
        let url = format!("https://{}/{}/", host, OCI_VERSION_2);
        let res = reqwest::get(&url).await?;
        let disthdr = res.headers().get(OCI_VERSION_KEY);
        let version = disthdr
            .ok_or_else(|| anyhow::format_err!("no header {} found", OCI_VERSION_KEY))?
            .to_str()?
            .to_owned();
        Ok(version)
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
    async fn test_pull_manifest() {
        // Currently, pull_manifest does not perform Authz, so this will fail.
        let c = Client::default();
        let res = c
            .pull_manifest("webassembly.azurecr.io/hello:v1".to_owned())
            .await
            .expect_err("pull manifest should fail");
    }
}
