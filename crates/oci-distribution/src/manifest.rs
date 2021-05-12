//! OCI Manifest
use std::collections::HashMap;

/// The mediatype for WASM layers.
pub const WASM_LAYER_MEDIA_TYPE: &str = "application/vnd.wasm.content.layer.v1+wasm";
/// The mediatype for a WASM image config.
pub const WASM_CONFIG_MEDIA_TYPE: &str = "application/vnd.wasm.config.v1+json";
/// The mediatype for an OCI manifest.
pub const IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.docker.distribution.manifest.v2+json";
/// The mediatype for an image config (manifest).
pub const IMAGE_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
/// The mediatype that Docker uses for image configs.
pub const IMAGE_DOCKER_CONFIG_MEDIA_TYPE: &str = "application/vnd.docker.container.image.v1+json";
/// The mediatype for a layer.
pub const IMAGE_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";
/// The mediatype for a layer that is gzipped.
pub const IMAGE_LAYER_GZIP_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
/// The mediatype that Docker uses for a layer that is gzipped.
pub const IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE: &str =
    "application/vnd.docker.image.rootfs.diff.tar.gzip";
/// The mediatype for a layer that is nondistributable.
pub const IMAGE_LAYER_NONDISTRIBUTABLE_MEDIA_TYPE: &str =
    "application/vnd.oci.image.layer.nondistributable.v1.tar";
/// The mediatype for a layer that is nondistributable and gzipped.
pub const IMAGE_LAYER_NONDISTRIBUTABLE_GZIP_MEDIA_TYPE: &str =
    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip";

// TODO: Annotation key constants. https://github.com/opencontainers/image-spec/blob/master/annotations.md#pre-defined-annotation-keys

/// The OCI manifest describes an OCI image.
///
/// It is part of the OCI specification, and is defined here:
/// https://github.com/opencontainers/image-spec/blob/master/manifest.md
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OciManifest {
    /// This is a schema version.
    ///
    /// The specification does not specify the width of this integer.
    /// However, the only version allowed by the specification is `2`.
    /// So we have made this a u8.
    pub schema_version: u8,

    /// This is an optional media type describing this manifest.
    ///
    /// It is reserved for compatibility, but the specification does not seem
    /// to recommend setting it.
    pub media_type: Option<String>,

    /// The image configuration.
    ///
    /// This object is required.
    pub config: OciDescriptor,

    /// The OCI image layers
    ///
    /// The specification is unclear whether this is required. We have left it
    /// required, assuming an empty vector can be used if necessary.
    pub layers: Vec<OciDescriptor>,

    /// The annotations for this manifest
    ///
    /// The specification says "If there are no annotations then this property
    /// MUST either be absent or be an empty map."
    /// TO accomodate either, this is optional.
    pub annotations: Option<HashMap<String, String>>,
}

impl Default for OciManifest {
    fn default() -> Self {
        OciManifest {
            schema_version: 2,
            media_type: None,
            config: OciDescriptor::default(),
            layers: vec![],
            annotations: None,
        }
    }
}

/// Versioned provides a struct with the manifest's schemaVersion and mediaType.
/// Incoming content with unknown schema versions can be decoded against this
/// struct to check the version.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Versioned {
    /// schema_version is the image manifest schema that this image follows
    pub schema_version: i32,

    /// media_type is the media type of this schema.
    pub media_type: Option<String>,
}

/// The OCI descriptor is a generic object used to describe other objects.
///
/// It is defined in the OCI Image Specification:
/// https://github.com/opencontainers/image-spec/blob/master/descriptor.md#properties
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OciDescriptor {
    /// The media type of this descriptor.
    ///
    /// Layers, config, and manifests may all have descriptors. Each
    /// is differentiated by its mediaType.
    ///
    /// This REQUIRED property contains the media type of the referenced
    /// content. Values MUST comply with RFC 6838, including the naming
    /// requirements in its section 4.2.
    pub media_type: String,
    /// The SHA 256 or 512 digest of the object this describes.
    ///
    /// This REQUIRED property is the digest of the targeted content, conforming
    /// to the requirements outlined in Digests. Retrieved content SHOULD be
    /// verified against this digest when consumed via untrusted sources.
    pub digest: String,
    /// The size, in bytes, of the object this describes.
    ///
    /// This REQUIRED property specifies the size, in bytes, of the raw
    /// content. This property exists so that a client will have an expected
    /// size for the content before processing. If the length of the retrieved
    /// content does not match the specified length, the content SHOULD NOT be
    /// trusted.
    pub size: i64,
    /// This OPTIONAL property specifies a list of URIs from which this
    /// object MAY be downloaded. Each entry MUST conform to RFC 3986.
    /// Entries SHOULD use the http and https schemes, as defined in RFC 7230.
    pub urls: Option<Vec<String>>,

    /// This OPTIONAL property contains arbitrary metadata for this descriptor.
    /// This OPTIONAL property MUST use the annotation rules.
    /// https://github.com/opencontainers/image-spec/blob/master/annotations.md#rules
    pub annotations: Option<HashMap<String, String>>,
}

impl Default for OciDescriptor {
    fn default() -> Self {
        OciDescriptor {
            media_type: IMAGE_CONFIG_MEDIA_TYPE.to_owned(),
            digest: "".to_owned(),
            size: 0,
            urls: None,
            annotations: None,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    const TEST_MANIFEST: &str = r#"{
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "size": 2,
            "digest": "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        },
        "layers": [
            {
                "mediaType": "application/vnd.wasm.content.layer.v1+wasm",
                "size": 1615998,
                "digest": "sha256:f9c91f4c280ab92aff9eb03b279c4774a80b84428741ab20855d32004b2b983f",
                "annotations": {
                    "org.opencontainers.image.title": "module.wasm"
                }
            }
        ]
    }
    "#;

    #[test]
    fn test_manifest() {
        let manifest: OciManifest = serde_json::from_str(TEST_MANIFEST).expect("parsed manifest");
        assert_eq!(2, manifest.schema_version);
        assert_eq!(
            Some(IMAGE_MANIFEST_MEDIA_TYPE.to_owned()),
            manifest.media_type
        );
        let config = manifest.config;
        // Note that this is the Docker config media type, not the OCI one.
        assert_eq!(IMAGE_DOCKER_CONFIG_MEDIA_TYPE.to_owned(), config.media_type);
        assert_eq!(2, config.size);
        assert_eq!(
            "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a".to_owned(),
            config.digest
        );

        assert_eq!(1, manifest.layers.len());
        let wasm_layer = &manifest.layers[0];
        assert_eq!(1_615_998, wasm_layer.size);
        assert_eq!(WASM_LAYER_MEDIA_TYPE.to_owned(), wasm_layer.media_type);
        assert_eq!(
            1,
            wasm_layer
                .annotations
                .as_ref()
                .expect("annotations map")
                .len()
        );
    }
}
