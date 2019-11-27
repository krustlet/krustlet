# Krustlet: Kubernetes Kubelet in Rust for running WASM

**This project is highly experimental**

Krustlet acts as a Kubelet by listening on the event stream for new pod requests that match a particular set of node pins.

## Building

We recommend using [just](https://github.com/casey/just) to build. But you can just use `cargo` if you want:

```console
$ just build
$ cargo build
```

Building a Docker image is easy, too:

```console
$ just dockerize
```

That will take a LOOONG time the first build, but the layer cache will make it much faster from then on.

## Running

Again, we recommend `just`, but you can use `cargo`:

```
$ just run
$ cargo run
```

## Scheduling Pods on the Krustlet

The krustlet listens for wasm32 and wasm-wasi:

- TBD