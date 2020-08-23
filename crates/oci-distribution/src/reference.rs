use std::convert::{Into, TryFrom};
use std::str::FromStr;

/// An OCI image reference
///
/// Parsing references in the following formats is supported:
/// - `<registry>/<repository>`
/// - `<registry>/<repository>:<tag>`
/// - `<registry>/<repository>@<digest>`
/// - `<registry>/<repository>:<tag>@<digest>`
///
/// # Examples
///
/// Parsing a tagged image reference:
/// ```
/// use oci_distribution::Reference;
///
/// let reference: Reference = "docker.io/library/hello-world:latest".parse().unwrap();
///
/// assert_eq!("docker.io", reference.registry());
/// assert_eq!("library/hello-world", reference.repository());
/// assert_eq!(Some("latest"), reference.tag());
/// assert_eq!(None, reference.digest());
/// ```
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
        let first_colon = string[repo_start + 1..].find(':').map(|i| repo_start + i);
        let digest_start = string[repo_start + 1..]
            .find('@')
            .map(|i| repo_start + i + 1);
        let tag_start = match (digest_start, first_colon) {
            // Check if a colon comes before a digest delimeter, indicating
            // that image ref is in the form registry/repo:tag@digest
            (Some(ds), Some(fc)) => {
                if fc < ds {
                    Some(fc)
                } else {
                    None
                }
            }
            // No digest delimeter was found but a colon is present, so ref
            // must be in the form registry/repo:tag
            (None, Some(fc)) => Some(fc),
            // No tag delimeter was found
            _ => None,
        }
        .map(|i| i + 1);
        let repo_end = match (digest_start, tag_start) {
            (Some(_), Some(ts)) => ts,
            (None, Some(ts)) => ts,
            (Some(ds), None) => ds,
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

impl FromStr for Reference {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Reference::try_from(s)
    }
}

impl Into<String> for Reference {
    fn into(self) -> String {
        self.whole
    }
}

#[cfg(test)]
mod test {
    use super::*;

    mod parse {
        use super::*;

        fn must_parse(image: &str) -> Reference {
            Reference::try_from(image).expect("could not parse reference")
        }

        fn validate_registry_and_repository(reference: &Reference) {
            assert_eq!(reference.registry(), "webassembly.azurecr.io");
            assert_eq!(reference.repository(), "hello");
        }

        fn validate_tag(reference: &Reference) {
            assert_eq!(reference.tag(), Some("v1"));
        }

        fn validate_digest(reference: &Reference) {
            assert_eq!(
                reference.digest(),
                Some("sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9")
            );
        }

        #[test]
        fn owned_string() {
            let reference = Reference::try_from("webassembly.azurecr.io/hello:v1".to_owned())
                .expect("could not parse reference");

            validate_registry_and_repository(&reference);
            validate_tag(&reference);
            assert_eq!(reference.digest(), None);
        }

        #[test]
        fn tag_only() {
            let reference = must_parse("webassembly.azurecr.io/hello:v1");

            validate_registry_and_repository(&reference);
            validate_tag(&reference);
            assert_eq!(reference.digest(), None);
        }

        #[test]
        fn digest_only() {
            let reference = must_parse("webassembly.azurecr.io/hello@sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9");

            validate_registry_and_repository(&reference);
            validate_digest(&reference);
            assert_eq!(reference.tag(), None);
        }

        #[test]
        fn tag_and_digest() {
            let reference = must_parse("webassembly.azurecr.io/hello:v1@sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9");

            validate_registry_and_repository(&reference);
            validate_tag(&reference);
            validate_digest(&reference);
        }

        #[test]
        fn no_tag_or_digest() {
            let reference = must_parse("webassembly.azurecr.io/hello");

            validate_registry_and_repository(&reference);
            assert_eq!(reference.tag(), None);
            assert_eq!(reference.digest(), None);
        }

        #[test]
        fn missing_slash_char() {
            Reference::try_from("webassembly.azurecr.io:hello")
                .expect_err("no slash should produce an error");
        }
    }
}
