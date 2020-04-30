# Krustlet: Kubernetes Kubelet in Rust for running WASM

:construction: :construction: **This project is highly experimental.** :construction: :construction:
It should not be used in production workloads.

Krustlet acts as a Kubelet by listening on the event stream for new pods that
the scheduler assigns to it based on specific Kubernetes
[tolerations](https://kubernetes.io/docs/concepts/configuration/taint-and-toleration/).

The default implementation of Krustlet listens for the architecture `wasm32-wasi` and schedules
those workloads to run in a `wasmtime`-based runtime instead of a container runtime.

## Documentation

If you're new to the project, get started with [the introduction](docs/intro/README.md). For more
in-depth information about Krustlet, plunge right into the [topic guides](docs/topics/README.md).

Looking for the developer guide? [Start here](docs/community/developers.md). If you are interested
in the project, please feel free to join us in our weekly public call [on
Zoom](https://zoom.us/j/200921512?pwd=N3hBblRaUE1FL3luVkJ6ZzZsM0NIUT09) at 9:00 am Pacific Time
every Monday

## Code of Conduct

This project has adopted the [Microsoft Open Source Code of
Conduct](https://opensource.microsoft.com/codeofconduct/).

For more information see the [Code of Conduct
FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or contact
[opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.
