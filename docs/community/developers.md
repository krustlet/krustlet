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
$ just
$ cargo build
```

Building a Docker image is easy, too:

```console
$ just dockerize
```

That will take a LOOONG time the first build, but the layer cache will make it much faster from then on.

## Running

To run Krustlet locally, you can run

```console
$ just run-wasi
```

Before startup, this command will delete any nodes in your Kubernetes cluster named "krustlet", so make sure you're
running this in a test environment.

Note that if you are running krustlet locally, calls to `kubectl log` and `kubectl exec` will result in errors.

## Creating your own Kubelets with Krustlet

If you want to create your own Kubelet based on Krustlet, all you need to do is implement a `Provider`.

See `src/krustlet-*.rs` and their corresponding provider implementation in `crates/*-provider` to get started.
