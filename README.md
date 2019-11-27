# Krustlet: Kubernetes Kubelet in Rust for running WASM

**This project is highly experimental**

Krustlet acts as a Kubelet by listening on the event stream for new pod requests that match a particular set of node pins.

## Building

```
cargo build
```

## Running

```
cargo run
```

## Scheduling Pods on the Krustlet

The krustlet listens for the following node pins:

- TBD