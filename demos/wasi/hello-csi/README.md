# Hello World Rust for WASI

This is a variant of the hello world example, demonstrating how to use Krustlet
with the [Container Storage Interface](https://docs.krustlet.dev/topics/csi).

A simple hello world example in Rust that will print:

- The environment variables available to the process
- Text to both stdout and stderr.
- Any args passed to the process

It is meant to be a simple demo for the wasi-provider with Krustlet.

## Running the example

First create the pod, storageclass, and persistentvolumeclaim:

```shell
$ kubectl apply -f k8s.yaml
```

You should then be able to get the logs and see the output from the wasm module
run:

```shell
$ kubectl logs hello-world-wasi-rust
hello from stdout!
hello from stderr!
FOO=bar
CONFIG_MAP_VAL=cool stuff
POD_NAME=hello-world-wasi-rust
Args are: []
```

## Building from Source

If you want to compile the demo and inspect it, you'll need to do the following.

### Prerequisites

You'll need to have Rust installed with `wasm32-wasi` target installed:

```shell
$ rustup target add wasm32-wasi
```

If you don't have Krustlet with the WASI provider running locally, see the
instructions in the [tutorial](https://docs.krustlet.dev/intro/tutorial03) for running
locally.

You will also need to register the [host-path CSI
driver](https://github.com/kubernetes-csi/csi-driver-host-path). Details on how
to register the driver can be found in the [CSI HOWTO
guide](https://docs.krustlet.dev/howto/csi).

### Building

Run:

```shell
$ cd ../hello-world-rust
$ cargo build --target wasm32-wasi --release
```

### Pushing

Detailed instructions for pushing a module can be found [here](https://docs.krustlet.dev/intro/tutorial02.md).

We hope to improve and streamline the build and push process in the future.
However, for test purposes, the image has been pushed to the `webassembly`
Azure Container Registry.
