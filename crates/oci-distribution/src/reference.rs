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
            (Some(ds), Some(fc)) => match fc < ds {
                true => Some(fc),
                false => None,
            },
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
    use rstest::rstest;
    use std::convert::TryInto;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ParseResult {
        registry: String,
        repository: String,
        tag: Option<String>,
        digest: Option<String>,
    }

    impl ParseResult {
        fn new<'a>(registry: &str, repository: &str) -> Self {
            Self {
                registry: registry.to_owned(),
                repository: repository.to_owned(),
                tag: None,
                digest: None,
            }
        }

        fn empty() -> Self {
            Self::new("", "")
        }

        fn with_tag(&mut self, tag: &str) -> Self {
            self.tag = Some(tag.to_owned());
            self.to_owned()
        }

        fn with_digest(&mut self, digest: &str) -> Self {
            self.digest = Some(digest.to_owned());
            self.to_owned()
        }
    }

    #[rstest(
        image, expected,
        case::owned_string(
            "webassembly.azurecr.io/hello:v1".to_owned(),
            ParseResult::new("webassembly.azurecr.io", "hello")
                .with_tag("v1"),
        ),
        case::tag(
            "webassembly.azurecr.io/hello:v1",
            ParseResult::new("webassembly.azurecr.io", "hello")
                .with_tag("v1"),
        ),
        case::digest(
            "webassembly.azurecr.io/hello@sha256:51d9b231d5129e3ffc267c9d455c49d789bf3167b611a07ab6e4b3304c96b0e7",
            ParseResult::new("webassembly.azurecr.io", "hello")
                .with_digest("sha256:51d9b231d5129e3ffc267c9d455c49d789bf3167b611a07ab6e4b3304c96b0e7"),
        ),
        case::tag_and_digest(
            "webassembly.azurecr.io/hello:v1@sha256:51d9b231d5129e3ffc267c9d455c49d789bf3167b611a07ab6e4b3304c96b0e7",
            ParseResult::new("webassembly.azurecr.io", "hello")
                .with_tag("v1")
                .with_digest("sha256:51d9b231d5129e3ffc267c9d455c49d789bf3167b611a07ab6e4b3304c96b0e7"),
        ),
        case::no_tag_or_digest(
            "webassembly.azurecr.io/hello",
            ParseResult::new("webassembly.azurecr.io", "hello"),
        ),
        #[should_panic(expected = "parsing failed: Failed to parse reference string \'webassembly.azurecr.io:hello\'. Expected at least one slash (/)")]
        case::missing_slash(
            "webassembly.azurecr.io:hello",
            ParseResult::empty(),
        ),
        #[should_panic(expected = "parsing failed: Failed to parse reference string \'\'. Expected at least one slash (/)")]
        case::empty(
            "",
            ParseResult::empty(),
        ),
        ::trace
    )]
    fn parse<T>(image: T, expected: ParseResult)
    where
        T: TryInto<Reference>,
        T::Error: Into<anyhow::Error>,
    {
        let r: Reference = image
            .try_into()
            .map_err(Into::into)
            .expect("parsing failed");

        assert_eq!(
            ParseResult {
                registry: r.registry().to_owned(),
                repository: r.repository().to_owned(),
                tag: r.tag().map(|t| t.to_owned()),
                digest: r.digest().map(|d| d.to_owned()),
            },
            expected
        );
    }
}
