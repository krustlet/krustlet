use std::convert::{Into, TryFrom};

/// An OCI image reference
///
/// currently, the library only accepts modules tagged in the following structure:
/// <registry>/<repository>, <registry>/<repository>:<tag>, or <registry>/<repository>@<digest>
/// for example: webassembly.azurecr.io/hello:v1 or webassembly.azurecr.io/hello
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Reference {
    whole: String,
    repo_start: usize,
    repo_end: usize,
    tag_start: Option<usize>,
    digest_start: Option<usize>,
}

impl std::fmt::Debug for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.whole)
    }
}

impl std::fmt::Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.whole)
    }
}

impl Reference {
    /// Get the original reference.
    pub fn whole(&self) -> &str {
        &self.whole
    }

    /// Get the registry name.
    pub fn registry(&self) -> &str {
        &self.whole[..self.repo_start]
    }

    /// Get the repository (a.k.a the image name) of this reference
    pub fn repository(&self) -> &str {
        &self.whole[self.repo_start + 1..self.repo_end]
    }

    /// Get the tag for this reference.
    pub fn tag(&self) -> Option<&str> {
        match (self.digest_start, self.tag_start) {
            (Some(d), Some(t)) => Some(&self.whole[t + 1..d]),
            (None, Some(t)) => Some(&self.whole[t + 1..]),
            _ => None,
        }
    }

    /// Get the digest for this reference.
    pub fn digest(&self) -> Option<&str> {
        match self.digest_start {
            Some(c) => Some(&self.whole[c + 1..]),
            None => None,
        }
    }
}

impl TryFrom<String> for Reference {
    type Error = anyhow::Error;
    fn try_from(string: String) -> Result<Self, Self::Error> {
        let repo_start = string.find('/').ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to parse reference string '{}'. Expected at least one slash (/)",
                string
            )
        })?;
        let digest_start = string[repo_start + 1..].find('@').map(|i| repo_start + i + 1);
        let mut tag_start = string[repo_start + 1..].find(':').map(|i| repo_start + i + 1);

        let repo_end = match (digest_start, tag_start) {
            (Some(d), Some(t)) => {
                if t > d {
                    // tag_start is after digest_start, so no tag is actually present
                    tag_start = None;
                    d
                } else {
                    t
                }
            },
            (Some(d), None) => d,
            (None, Some(t)) => t,
            (None, None) => string.len(),
        };

        Ok(Reference {
            whole: string,
            repo_start,
            repo_end,
            tag_start,
            digest_start,
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
        // Tag only (String)
        let reference = Reference::try_from("webassembly.azurecr.io/hello:v1".to_owned())
            .expect("Could not parse reference");

        assert_eq!(reference.registry(), "webassembly.azurecr.io");
        assert_eq!(reference.repository(), "hello");
        assert_eq!(reference.tag(), Some("v1"));

        // Tag only (&str)
        let reference = Reference::try_from("webassembly.azurecr.io/hello:v1")
            .expect("Could not parse reference");

        assert_eq!(reference.registry(), "webassembly.azurecr.io");
        assert_eq!(reference.repository(), "hello");
        assert_eq!(reference.tag(), Some("v1"));

        // Digest only
        let reference = Reference::try_from("webassembly.azurecr.io/hello@sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9")
            .expect("Could not parse reference");

        assert_eq!(reference.registry(), "webassembly.azurecr.io");
        assert_eq!(reference.repository(), "hello");
        assert_eq!(reference.tag(), None);
        assert_eq!(reference.digest(), Some("sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9"));

        // Tag and digest
        let reference = Reference::try_from("webassembly.azurecr.io/hello:v1@sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9")
        .expect("Could not parse reference");

        assert_eq!(reference.registry(), "webassembly.azurecr.io");
        assert_eq!(reference.repository(), "hello");
        assert_eq!(reference.tag(), Some("v1"));
        assert_eq!(reference.digest(), Some("sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9"));

        // No tag or digest
        let reference =
            Reference::try_from("webassembly.azurecr.io/hello").expect("Could not parse reference");

        assert_eq!(reference.registry(), "webassembly.azurecr.io");
        assert_eq!(reference.repository(), "hello");
        assert_eq!(reference.tag(), None);

        // Missing slash character
        Reference::try_from("webassembly.azurecr.io:hello")
            .expect_err("No slash should produce an error");
    }
}
