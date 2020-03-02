# Krustlet: Kubernetes Kubelet in Rust for running WASM

**This project is highly experimental.** It is just a proof of concept, and you
should not use it in production.

Krustlet acts as a Kubelet by listening on the event stream for new pod requests
that match a particular set of node selectors.

The default implementation of Krustlet listens for the architecture
`wasm32-wasi` and schedules those workloads to run in a `wasmtime`-based runtime
instead of a container runtime.

## Documentation

If you're new to the project, get started with [the introduction](docs/intro/README.md). For more in-depth information
about Krustlet, plunge right into the [topic guides](docs/topics/README.md).

Looking for the developer guide? [Start here](docs/community/developers.md).

## Code of Conduct

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).

For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/)
or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.
