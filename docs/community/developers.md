# Developer guide

This guide explains how to set up your environment for developing Krustlet.

## Prerequisites

To build krustlet, you will need

- The latest stable version of Rust
- The latest version of [just](https://github.com/casey/just)
- openssl (Or use the [`rustls-tls`](#building-without-openssl) feature)
- git

If you want to test krustlet, you will also require

- A Kubernetes cluster
- The latest version of
  [kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl/)

If you want to compile your own WebAssembly modules and upload them to a
registry, you'll need [wasm-to-oci](https://github.com/engineerd/wasm-to-oci).

If you want to build the Docker image, you'll need
[Docker](https://docs.docker.com/install/).

## Building

We use `just` to build our programs, but you can use `cargo` if you want:

```console
$ just build
```

### Building without openssl

If you are on a system that doesn't have OpenSSL (or has the incorrect version),
you have the option to build Krustlet using the Rustls project (Rust native TLS
implementation):

```console
$ just build --no-default-features --features rustls-tls
```

The same flags can be passed to `just run` if you want to just [run](#running)
the project instead.

#### Caveats

The underlying dependencies for Rustls do not support certs with IP SANs
(subject alternate names). Because of this, the serving certs requested during
bootstrap will not work for local development options like minikube or KinD as
they do not have an FQDN

### Building on WSL (Windows Subsystem for Linux)

You can build Krustlet on WSL but will need a few prerequisites that aren't
included in the Ubuntu distro in the Microsoft Store:

```console
$ sudo apt install build-essential libssl-dev pkg-config
```

**NOTE:** We've had mixed success developing Krustlet on WSL.  It has been
successfully run on WSL2 using the WSL2-enabled Docker Kubernetes or Azure
Kubernetes.  If you're on WSL1 you may be better off running in a full Linux VM
under Hyper-V.

### Building on Windows

As of version 0.4, we have support for building on Windows. For convenience
sake, there is a windows version of the justfile called `justfile-windows`. This
justfile uses PowerShell and has the proper flags set for Windows builds. To use
it, you'll have to specify the justfile using the `--justfile` flag like so:

```console
$ just --justfile justfile-windows build
```

It has all the same targets as the normal justfile, however, the `test` target
runs a little differently than the normal target due to how we use feature
flags. This means there will be some spurious warning output from `clippy`, but
the tests will run.

**NOTE:** Windows builds use the `rustls` library, which means there are some
things to be aware of. See the [caveats](#caveats) section for more details

## Running

The default included runtime with Krustlet is `wasi`.

The `wasi` runtime uses a project called
[`wasmtime`](https://github.com/bytecodealliance/wasmtime). wasmtime is a
standalone JIT-style host runtime for WebAssembly modules. It is focused
primarily on standards compliance with the WASM specification as it relates to
[WASI](https://wasi.dev/). If your WebAssembly module complies with the
[WebAssembly specification](https://github.com/WebAssembly/spec), wasmtime can
run it.

Before startup, this command will delete any nodes in your Kubernetes cluster
named with your hostname, so make sure you're running this in a test
environment.

If you want to interact with the kubelet (for things like `kubectl logs` and
`kubectl exec`), you'll likely need to set a specific KRUSTLET_NODE_IP that
krustlet will be available at. Otherwise, calls to the kubelet will result in
errors. This may differ from machine to machine. For example, with Minikube on a
Mac, you'll have an interface called `bridge0` which the cluster can talk to. So
your node IP should be that IP address.

To set the node IP, run:

```console
$ export KRUSTLET_NODE_IP=<the ip address>
```

## Testing

Krustlet contains both integration and unit tests. For convenience, there are
`just` targets for running one or the other.

For unit tests:

```console
$ just test
```

For the integration tests, start a WASI node in a separate terminal before
running the tests.

In terminal 1:

```console
$ just run-wasi
```

And in terminal 2:

```console
$ just test-e2e
```

You can run the integration tests without creating additional terminals or
manually running the kubelets by running:

```console
$ just test-e2e-standalone
```

This:

- Bootstraps and approves certificates if necessary
- Runs the WASI kubelet in the background
- Runs the integration tests
- Terminates the kubelets when the integration tests complete
- Reports test failures, and saves the kubelet logs if any tests failed

You **will** still need to set `KRUSTLET_NODE_IP` because the tester doesn't
know what kind of Kubernetes cluster you're using and so doesn't know how to
infer a node IP.

_WARNING:_ The standalone integration tester has not been, er, tested on
Windows. Hashtag irony.

### Integration test debris

There are some failure modes - for example image pull timeout - where the
integration tests are not able to complete cleanup of their resources.
Specifically you can sometimes get pods stuck in `Terminating`, which prevents
namespace cleanup and causes the next test run to break.

You can forcibly clean up such debris by running `cargo run --bin podsmiter`.
You may need to wait a couple of minutes after pod deletion for the namespaces
to be collected.

## Creating your own Kubelets with Krustlet

If you want to create your own Kubelet based on Krustlet, all you need to do is
implement a `Provider`.

See `src/krustlet-wasi.rs` and its corresponding provider implementation in
`crates/wasi-provider` to get started.
