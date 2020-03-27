use std::convert::{Into, TryFrom};

/// An OCI image reference
///
/// currently, the library only accepts modules tagged in the following structure:
/// <registry>/<repository>:<tag>
/// for example: webassembly.azurecr.io/hello:v1
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Reference {
    whole: String,
    slash: usize,
    colon: usize,
}

impl Reference {
    /// Get the original reference.
    pub fn whole(&self) -> &str {
        &self.whole
    }

    /// Get the registry name.
    pub fn registry(&self) -> &str {
        &self.whole[..self.slash]
    }

    /// Get the repository (a.k.a the image name) of this reference
    pub fn repository(&self) -> &str {
        &self.whole[self.slash + 1..self.colon]
    }

    /// Get the tag for this reference.
    pub fn tag(&self) -> &str {
        &self.whole[self.colon + 1..]
    }

    /// Convert a Reference to a v2 manifest URL.
    pub fn to_v2_manifest_url(&self, protocol: &str) -> String {
        format!(
            "{}://{}/v2/{}/manifests/{}",
            protocol,
            self.registry(),
            self.repository(),
            self.tag()
        )
    }

    /// Convert a Reference to a v2 blob (layer) URL.
    pub fn to_v2_blob_url(&self, protocol: &str, digest: &str) -> String {
        format!(
            "{}://{}/v2/{}/blobs/{}",
            protocol,
            self.registry(),
            self.repository(),
            digest
        )
    }
}

impl TryFrom<String> for Reference {
    type Error = anyhow::Error;
    fn try_from(string: String) -> Result<Self, Self::Error> {
        let slash = string.find('/').ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to parse reference string '{}'. Expected at least one slash (/)",
                string
            )
        })?;
        let colon = string[slash + 1..].find(':').ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to parse reference string {}. Expected exactly one colon (:)",
                string
            )
        })?;
        Ok(Reference {
            whole: string,
            slash,
            colon: slash + 1 + colon,
        })
    }
}

impl TryFrom<&str> for Reference {
    type Error = anyhow::Error;
    fn try_from(string: &str) -> Result<Self, Self::Error> {
        TryFrom::try_from(string.to_owned())
    }
}

impl Into<String> for Reference {
    fn into(self) -> String {
        self.whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correctly_parses_string() {
        let reference = Reference::try_from("webassembly.azurecr.io/hello:v1".to_owned())
            .expect("Could not parse reference");

        assert_eq!(reference.registry(), "webassembly.azurecr.io");
        assert_eq!(reference.repository(), "hello");
        assert_eq!(reference.tag(), "v1");

        let reference = Reference::try_from("webassembly.azurecr.io/hello:v1")
            .expect("Could not parse reference");

        assert_eq!(reference.registry(), "webassembly.azurecr.io");
        assert_eq!(reference.repository(), "hello");
        assert_eq!(reference.tag(), "v1");

        Reference::try_from("webassembly.azurecr.io/hello")
            .expect_err("No colon should produce an error");
        Reference::try_from("webassembly.azurecr.io:hello")
            .expect_err("No slash should produce an error");
    }

    #[test]
    fn test_to_v2_manifest() {
        let reference = Reference::try_from("webassembly.azurecr.io/hello:v1".to_owned())
            .expect("Could not parse reference");
        assert_eq!(
            "https://webassembly.azurecr.io/v2/hello/manifests/v1",
            reference.to_v2_manifest_url("https")
        );
    }
}
