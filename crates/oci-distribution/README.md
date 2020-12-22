# OCI Distribution

[![oci-distribution documentation](https://docs.rs/oci-distribution/badge.svg)](https://docs.rs/oci-distribution)

This Rust library implements the
[OCI Distribution specification](https://github.com/opencontainers/distribution-spec/blob/master/spec.md),
which is the protocol that Docker Hub and other container registries use.

The immediate goal of this crate is to provide a way to pull WASM modules from
a Docker registry. However, our broader goal is to implement the spec in its
entirety.
