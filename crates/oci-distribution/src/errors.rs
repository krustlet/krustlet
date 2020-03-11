/// The OCI specification defines a specific error format.
///
/// This struct represents that error format, which is formally described here:
/// https://github.com/opencontainers/distribution-spec/blob/master/spec.md#errors-2
#[derive(serde::Deserialize, Debug)]
pub struct OciError {
    pub code: OciErrorCode,
    pub message: String,
    pub detail: serde_json::Value,
}

impl std::error::Error for OciError {
    fn description(&self) -> &str {
        self.message.as_str()
    }
}
impl std::fmt::Display for OciError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OCI API error: {}", self.message.as_str())
    }
}

#[derive(serde::Deserialize)]
pub struct OciEnvelope {
    pub errors: Vec<OciError>,
}

#[derive(serde::Deserialize, Debug, PartialEq)]
pub enum OciErrorCode {
    /// Blob unknown to registry
    ///
    /// This error MAY be returned when a blob is unknown to the registry in a specified
    /// repository. This can be returned with a standard get or if a manifest
    /// references an unknown layer during upload.
    BLOB_UNKNOWN,
    /// Blob upload is invalid
    ///
    /// The blob upload encountered an error and can no longer proceed.
    BLOB_UPLOAD_INVALID,
    /// Blob upload is unknown to registry
    BLOB_UPLOAD_UNKNOWN,
    /// Provided digest did not match uploaded content.
    DIGEST_INVALID,
    /// Blob is unknown to registry
    MANIFEST_BLOB_UNKNOWN,
    /// Manifest is invalid
    ///
    /// During upload, manifests undergo several checks ensuring validity. If
    /// those checks fail, this error MAY be returned, unless a more specific
    /// error is included. The detail will contain information the failed
    /// validation.
    MANIFEST_INVALID,
    /// Manifest unknown
    ///
    /// This error is returned when the manifest, identified by name and tag is unknown to the repository.
    MANIFEST_UNKNOWN,
    /// Manifest failed signature validation
    MANIFEST_UNVERIFIED,
    /// Invalid repository name
    NAME_INVALID,
    /// Repository name is not known
    NAME_UNKNOWN,
    // Provided length did not match content length
    SIZE_INVALID,
    /// Manifest tag did not match URI
    TAG_INVALID,
    /// Authentication required.
    UNAUTHORIZED,
    // Requested access to the resource is denied
    DENIED,
    /// This operation is unsupported
    UNSUPPORTED,
}

#[cfg(test)]
mod test {
    use super::*;

    const example_error: &str = r#"
      {"errors":[{"code":"UNAUTHORIZED","message":"authentication required","detail":[{"Type":"repository","Name":"hello-wasm","Action":"pull"}]}]}
      "#;
    #[test]
    fn test_deserialize() {
        let envelope: OciEnvelope =
            serde_json::from_str(example_error).expect("parse example error");
        let e = &envelope.errors[0];
        assert_eq!(OciErrorCode::UNAUTHORIZED, e.code);
        assert_eq!("authentication required", e.message);
    }
}
