# Krustlet: Kubernetes Kubelet in Rust for running WASM

:construction: :construction: **This project is highly experimental.**
:construction: :construction: It should not be used in production workloads.

Krustlet acts as a Kubelet by listening on the event stream for new pods that
the scheduler assigns to it based on specific Kubernetes
[tolerations](https://kubernetes.io/docs/concepts/configuration/taint-and-toleration/).

The default implementation of Krustlet listens for the architecture
`wasm32-wasi` and schedules those workloads to run in a `wasmtime`-based runtime
instead of a container runtime.

## Documentation

If you're new to the project, get started with [the
introduction](docs/intro/README.md). For more in-depth information about
Krustlet, plunge right into the [topic guides](docs/topics/README.md).

Looking for the developer guide? [Start here](docs/community/developers.md).

## Community, discussion, contribution, and support

You can reach the Krustlet community and developers via the following channels:

- [Kubernetes Slack](https://kubernetes.slack.com):
  - [#krustlet](https://kubernetes.slack.com/messages/krustlet)
- Public Community Call on Mondays at 1:00 PM PT:
  - [Zoom](https://us04web.zoom.us/j/71695031152?pwd=T0g1d0JDZVdiMHpNNVF1blhxVC9qUT09)
  - Download the meeting calendar invite
    [here](https://raw.githubusercontent.com/deislabs/krustlet/master/docs/community/assets/community_meeting.ics)

## Code of Conduct

This project has adopted the [Microsoft Open Source Code of
Conduct](https://opensource.microsoft.com/codeofconduct/).

For more information see the [Code of Conduct
FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or contact
[opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional
questions or comments.
