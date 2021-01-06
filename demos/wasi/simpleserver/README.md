# WASM-WASI SimpleServer

This is a simple Rust server to demonstrate how to write a WASM-WASI long-running
binary.

## Simple Install

If you just want to install a long-running WASM-WASI workload, you can simply run
`kubectl apply -f simpleserver.yaml`.

## Build and Install

This assumes that you have read the documentation on Krustlet and installed the
`wasm32-wasi` Rust environment as well as `wasm-to-oci`.

> NOTE: In most cases you will want to copy the `simpleserver` directory out of
> the Krustlet codebase to get it to build quickly and correctly.
> `cp -a simpleserver ../../../`

### To build

Run `cargo build` in this directory.

The WASM binary will be written to `target/wasm32-wasi/debug/simpleserver.wasm`.

### To Upload

Use `wasm-to-oci` to write this to an OCI registry.

```console
$ wasm-to-oci push target/wasm32-wasi/debug/simpleserver.wasm MY_REGISTRY/simpleserver:v1.0.0
```

You will need to log into your container registry to do this. At this time,
DockerHub does not have a whitelist entry for WASM files, so you can't store
them in DockerHub.

### To Install

Update the `simpleserver.yaml` to point to the image that you pushed in the
previous step.

Use `kubectl apply -f simpleserver.yaml`

From there, you can check on the pod with `kubectl logs simpleserver`. You
should see log messages written to the console every five seconds or so.
