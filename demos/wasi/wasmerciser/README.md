# Exerciser for WASI

A simple WASM module designed to be driven using environment variables
and flags to exercise functionality that is useful for Krustlet integration
tests.

## Building from Source

If you want to compile the demo and inspect it, you'll need to do the following.

### Prerequisites

You'll need to have Rust installed with `wasm32-wasi` target installed:

```shell
$ rustup target add wasm32-wasi
```

If you don't have Krustlet with the WASI provider running locally, see
the instructions in the [tutorial](https://docs.krustlet.dev/intro/tutorial03) for
running locally.

### Building

Run:

```shell
$ cargo build --target wasm32-wasi --release
```

### Pushing

Detailed instructions for pushing a module can be found [here](https://docs.krustlet.dev/intro/tutorial02).

We hope to improve and streamline the build and push process in the future.
However, for test purposes, the image will be pushed to the `webassembly` Azure
Container Registry.
