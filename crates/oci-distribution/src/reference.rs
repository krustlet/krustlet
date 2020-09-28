use std::convert::{Into, TryFrom};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// NAME_TOTAL_LENGTH_MAX is the maximum total number of characters in a repository name.
const NAME_TOTAL_LENGTH_MAX: usize = 255;

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    DigestInvalidFormat,
    NameContainsUppercase,
    NameEmpty,
    NameNotCanonical,
    NameTooLong,
    ReferenceInvalidFormat,
    TagInvalidFormat,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::DigestInvalidFormat => write!(f, "invalid digest format"),
            ParseError::NameContainsUppercase => write!(f, "repository name must be lowercase"),
            ParseError::NameEmpty => write!(f, "repository name must have at least one component"),
            ParseError::NameNotCanonical => write!(f, "repository name must be canonical"),
            ParseError::NameTooLong => write!(
                f,
                "repository name must not be more than {} characters",
                NAME_TOTAL_LENGTH_MAX
            ),
            ParseError::ReferenceInvalidFormat => write!(f, "invalid reference format"),
            ParseError::TagInvalidFormat => write!(f, "invalid tag format"),
        }
    }
}

impl Error for ParseError {}

/// Reference provides a general type to represent any way of referencing images within an OCI registry.
///
/// # Examples
///
/// Parsing a tagged image reference:
///
/// ```
/// use oci_distribution::Reference;
///
/// let reference: Reference = "docker.io/library/hello-world:latest".parse().unwrap();
///
/// assert_eq!("docker.io/library/hello-world:latest", reference.whole().as_str());
/// assert_eq!("docker.io", reference.registry());
/// assert_eq!("library/hello-world", reference.repository());
/// assert_eq!(Some("latest"), reference.tag());
/// assert_eq!(None, reference.digest());
/// ```
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Reference {
    registry: String,
    repository: String,
    tag: Option<String>,
    digest: Option<String>,
}

impl Reference {
    /// registry returns the name of the registry.
    pub fn registry(&self) -> &str {
        &self.registry
    }

    /// repository returns the name of the repository.
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// tag returns the object's tag, if present.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// digest returns the object's digest, if present.
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }

    /// full_name returns the full repository name and path.
    fn full_name(&self) -> String {
        let mut path = PathBuf::new();
        path.push(self.registry());
        path.push(self.repository());
        path.to_str().unwrap_or("").to_owned()
    }

    /// whole returns the whole reference.
    pub fn whole(&self) -> String {
        let mut s = self.full_name();
        if let Some(t) = self.tag() {
            if s != "" {
                s.push_str(":");
            }
            s.push_str(t);
        }
        if let Some(d) = self.digest() {
            if s != "" {
                s.push_str("@");
            }
            s.push_str(d);
        }
        s
    }
}

impl std::fmt::Debug for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.whole())
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.whole())
    }
}

impl FromStr for Reference {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Reference::try_from(s)
    }
}

impl TryFrom<String> for Reference {
    type Error = ParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let repo_start = s
            .find('/')
            .ok_or_else(|| ParseError::ReferenceInvalidFormat)?;
        let first_colon = s[repo_start + 1..].find(':').map(|i| repo_start + i);
        let digest_start = s[repo_start + 1..].find('@').map(|i| repo_start + i + 1);
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
            (None, None) => s.len(),
        };

        let tag: Option<String> = match (digest_start, tag_start) {
            (Some(d), Some(t)) => Some(s[t + 1..d].to_string()),
            (None, Some(t)) => Some(s[t + 1..].to_string()),
            _ => None,
        };

        let digest: Option<String> = match digest_start {
            Some(c) => Some(s[c + 1..].to_string()),
            None => None,
        };

        let reference = Reference {
            registry: s[..repo_start].to_string(),
            repository: s[repo_start + 1..repo_end].to_string(),
            tag,
            digest,
        };

        if reference.repository().len() > NAME_TOTAL_LENGTH_MAX {
            return Err(ParseError::NameTooLong);
        }

        Ok(reference)
    }
}

impl TryFrom<&str> for Reference {
    type Error = ParseError;
    fn try_from(string: &str) -> Result<Self, Self::Error> {
        TryFrom::try_from(string.to_owned())
    }
}

impl Into<String> for Reference {
    fn into(self) -> String {
        self.whole()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    mod parse {
        use super::*;
        use rstest::rstest;

        #[rstest(input, registry, repository, tag, digest,
            case("webassembly.azurecr.io/hello:v1@sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9", "webassembly.azurecr.io", "hello", Some("v1"), Some("sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9")),
            case("webassembly.azurecr.io/hello@sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9", "webassembly.azurecr.io", "hello", None, Some("sha256:f29dba55022eec8c0ce1cbfaaed45f2352ab3fbbb1cdcd5ea30ca3513deb70c9")),
            case("webassembly.azurecr.io/hello:v1", "webassembly.azurecr.io", "hello", Some("v1"), None),
            case("webassembly.azurecr.io/hello", "webassembly.azurecr.io", "hello", None, None),
            case("webassembly.azurecr.io/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "webassembly.azurecr.io", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None, None)
        )]
        fn parse_good_reference(
            input: &str,
            registry: &str,
            repository: &str,
            tag: Option<&str>,
            digest: Option<&str>,
        ) {
            let reference = Reference::try_from(input).expect("could not parse reference");
            assert_eq!(registry, reference.registry());
            assert_eq!(repository, reference.repository());
            assert_eq!(tag, reference.tag());
            assert_eq!(digest, reference.digest());
        }

        #[rstest(input, err,
            case("webassembly.azurecr.io:hello", ParseError::ReferenceInvalidFormat),
            case("webassembly.azurecr.io/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", ParseError::NameTooLong)
        )]
        fn parse_bad_reference(input: &str, err: ParseError) {
            assert_eq!(Reference::try_from(input).unwrap_err(), err)
        }
    }
}
