# Hello World C for WASI

A simple hello world example in C that will print:

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
$ kubectl logs hello-world-wasi-c
hello from stdout!
hello from stderr!
FOO=bar
CONFIG_MAP_VAL=cool stuff
POD_NAME=hello-world-wasi-c
[]
```

## Building from Source

If you want to compile the demo and inspect it, you'll need to do the following.

### Prerequisites

Building WASI in C is easiest with the custom
[SDK](https://github.com/WebAssembly/wasi-sdk) from wasmtime. Feel free to go
down the rabbit hole of figuring things out with vanilla `clang` for your
own project should you so desire.

To install the SDK, follow the steps below, replacing `$OS_NAME` with your OS
(current choices are `linux` and `macos`):

```shell
$ wget https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-8/wasi-sysroot-8.0.tar.gz
$ tar -xzf wasi-sysroot-8.0.tar.gz
$ wget https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-8/wasi-sdk-8.0-${OS_NAME}.tar.gz
$ tar -xzf wasi-sdk-8.0-${OS_NAME}.tar.gz
```

If you don't have Krustlet with the WASI provider running locally, see the
instructions in the [tutorial](https://docs.krustlet.dev/intro/tutorial03) for running
locally.

### Building

Run:

```shell
$ ./wasi-sdk-8.0/bin/clang -v demo.c --sysroot ./wasi-sysroot -o demo.wasm
```

### Pushing

Detailed instructions for pushing a module can be found
[here](https://docs.krustlet.dev/intro/tutorial02).

We hope to improve and streamline the build and push process in the future.
However, for test purposes, the image has been pushed to the `webassembly`
Azure Container Registry.
