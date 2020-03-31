# Developer guide

This guide explains how to set up your environment for developing Krustlet.

## Prerequisites

To build krustlet, you will need

- The latest stable version of Rust
- The latest version of [just](https://github.com/casey/just)
- openssl
- git

If you want to test krustlet, you will also require

- A Kubernetes cluster
- The latest version of [kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl/)

If you want to compile your own WebAssembly modules and upload them to a registry, you'll need
[wasm-to-oci](https://github.com/engineerd/wasm-to-oci).

If you want to build the Docker image, you'll need [Docker](https://docs.docker.com/install/).

## Building

We use `just` to build our programs, but you can use `cargo` if you want:

```console
$ just build
```

## Running

There are two different runtimes available for Krustlet: `wascc` or `wasi`.

The `wascc` runtime is a secure WebAssembly host runtime, connecting "actors" and "capability providers" together to
connect your WebAssembly runtime to cloud-native services like message brokers, databases, or other external services
normally unavailable to the WebAssembly runtime.

The `wasi` runtime uses a project called [`wasmtime`](https://github.com/bytecodealliance/wasmtime). wasmtime is a
standalone JIT-style host runtime for WebAssembly modules. It is focused primarily on standards compliance with the WASM
specification as it relates to [WASI](https://wasi.dev/). If your WebAssembly module complies with the
[WebAssembly specification](https://github.com/WebAssembly/spec), wasmtime can run it.

Depending on which host runtime you want, choose one of either:

```console
$ just run-wascc
$ just run-wasi
```

Before startup, this command will delete any nodes in your Kubernetes cluster named with your hostname, so make sure
you're running this in a test environment.

If you want to interact with the kubelet (for things like `kubectl logs` and `kubectl exec`), you'll likely need to set
a specific KRUSTLET_NODE_IP that krustlet will be available at. Otherwise, calls to the kubelet will result in errors.
This may differ from machine to machine. For example, with Minikube on a Mac, you'll have an interface called `bridge0`
which the cluster can talk to. So your node IP should be that IP address.

To set the node IP, run:

```console
$ export KRUSTLET_NODE_IP=<the ip address>
```

## Testing

Krustlet contains both integration and unit tests. For convenience, there are `just` targets for running one or the other.

For unit tests:

```console
$ just test
```

For the integration tests, start a wascc and wasi node in separate terminals before running the tests.

In terminal 1:

```console
$ just run-wascc
```

In terminal 2:

```console
$ just run-wasi
```

And in terminal 3:

```
$ just test-e2e
```

## Creating your own Kubelets with Krustlet

If you want to create your own Kubelet based on Krustlet, all you need to do is implement a `Provider`.

See `src/krustlet-*.rs` and their corresponding provider implementation in `crates/*-provider` to get started.
