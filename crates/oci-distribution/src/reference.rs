use failure::format_err;
use std::convert::{Into, TryFrom};

// currently, the library only accepts modules tagged in the following structure:
// <registry>/<repository>:<tag>
// for example: webassembly.azurecr.io/hello:v1
#[derive(Clone, Debug)]
pub struct Reference {
    whole: String,
    slash: usize,
    colon: usize,
}

impl Reference {
    pub fn whole(&self) -> &str {
        &self.whole
    }

    pub fn registry(&self) -> &str {
        &self.whole[..self.slash]
    }

    pub fn repository(&self) -> &str {
        &self.whole[self.slash + 1..self.colon]
    }

    pub fn tag(&self) -> &str {
        &self.whole[self.colon + 1..]
    }

    pub fn to_v2_manifest_url(&self) -> String {
        format!(
            "https://{}/v2/{}/manifests/{}",
            self.registry(),
            self.repository(),
            self.tag()
        )
    }
}

impl TryFrom<String> for Reference {
    type Error = ();
    fn try_from(string: String) -> Result<Self, Self::Error> {
        let slash = string.find('/').ok_or(())?;
        let colon = string[slash + 1..].find(':').ok_or(())?;
        Ok(Reference {
            whole: string,
            slash,
            colon: slash + 1 + colon,
        })
    }
}

impl TryFrom<&str> for Reference {
    type Error = failure::Error;
    fn try_from(string: &str) -> Result<Self, Self::Error> {
        let slash = string.find('/').ok_or_else(|| {
            format_err!("Failed to pare {}. Expected at least one slash (/)", string)
        })?;
        let colon = string[slash + 1..].find(':').ok_or_else(|| {
            format_err!("failed to parse {}. Expected exactly one colon (:)", string)
        })?;
        Ok(Reference {
            whole: string.to_owned(),
            slash,
            colon: slash + 1 + colon,
        })
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
            reference.to_v2_manifest_url()
        );
    }
}
