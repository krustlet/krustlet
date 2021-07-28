# Hello World Rust for WASI

A simple hello world example in Rust that will print:

- The environment variables available to the process
- Text to both stdout and stderr.
- Any args passed to the process

It is meant to be a simple demo for the wasi-provider with Krustlet.

## Running the example

First create the pod and configmap:

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

### Building

Run:

```shell
$ cargo build --target wasm32-wasi --release
```

### Pushing

Detailed instructions for pushing a module can be found [here](https://docs.krustlet.dev/intro/tutorial02).

We hope to improve and streamline the build and push process in the future.
However, for test purposes, the image has been pushed to the `webassembly`
Azure Container Registry.
