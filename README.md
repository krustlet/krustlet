⚠️ This project is currently not actively maintained. Most of the other maintainers have moved on to other WebAssembly related projects. This project could definitely still be useful to anyone who wants to write a custom Kubelet and its sister project [Krator](https://github.com/krustlet/krator) is a state machine-based solution for writing Kubernetes controllers/operators in Rust. If anyone is interested in maintaining these projects, please feel free to reach out!


[![CII Best
Practices](https://bestpractices.coreinfrastructure.org/projects/5292/badge)](https://bestpractices.coreinfrastructure.org/projects/5292)

# Krustlet: Kubernetes Kubelet in Rust for running WASM

:postal_horn: Krustlet 1.0 coming soon!

Krustlet acts as a Kubelet by listening on the event stream for new pods that
the scheduler assigns to it based on specific Kubernetes
[tolerations](https://kubernetes.io/docs/concepts/configuration/taint-and-toleration/).

The default implementation of Krustlet listens for the architecture
`wasm32-wasi` and schedules those workloads to run in a `wasmtime`-based runtime
instead of a container runtime.

## Documentation

If you're new to the project, get started with [the
introduction](https://docs.krustlet.dev/intro). For more in-depth information about
Krustlet, plunge right into the [topic guides](https://docs.krustlet.dev/topics).

Looking for the developer guide? [Start here](https://docs.krustlet.dev/community/developers).

## Community, discussion, contribution, and support

You can reach the Krustlet community and developers via the following channels:

- [Kubernetes Slack](https://kubernetes.slack.com):
  - [#krustlet](https://kubernetes.slack.com/messages/krustlet)
- Public Community Call on Mondays at 1:00 PM PT:
  - [Zoom](https://us04web.zoom.us/j/71695031152?pwd=T0g1d0JDZVdiMHpNNVF1blhxVC9qUT09)
  - Download the meeting calendar invite
    [here](./community_meeting.ics)

## Code of Conduct

This project has adopted the [CNCF Code of
Conduct](https://github.com/cncf/foundation/blob/master/code-of-conduct.md).
