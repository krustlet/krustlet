# Experimental WASI HTTP

This example demonstrates how to build a WASM module that makes an HTTP request using the `wasi-experimental-http` crate. 

## Build and Install

This assumes that you have read the documentation on Krustlet and installed the
`wasm32-wasi` Rust environment as well as `wasm-to-oci`.

> NOTE: In most cases you will want to copy the `postman-echo` directory out of
> the Krustlet codebase to get it to build quickly and correctly.
> `cp -a postman-echo ../../../`

### To build

Run in this directory.

```shell
$ cargo wasi build --release
```

The WASM binary will be written to `target/wasm32-wasi/release/postman-echo.wasm`.

### To Upload

Use `wasm-to-oci` to write this to an OCI registry.

```console
$ wasm-to-oci push target/wasm32-wasi/release/postman-echo.wasm MY_REGISTRY/postman-echo:v1.0.0
```

You will need to log into your container registry to do this. At this time,
DockerHub does not have a whitelist entry for WASM files, so you can't store
them in DockerHub.

### To Install

Update the `k8s.yaml` to point to the image that you pushed in the
previous step.

Use `kubectl apply -f k8s.yaml`

Check the logs of the `postman-echo` pod:

```shell
$ kubectl logs --tail=1 postman-echo | python -m json.tool
```

Should print the echoed HTTP body similar to:

```json
{
    "args": {},
    "data": "I'm not superstitious, but I am a little stitious.",
    "files": {},
    "form": {},
    "headers": {
        "accept": "*/*",
        "content-length": "50",
        "content-type": "text/plain",
        "host": "postman-echo.com",
        "x-amzn-trace-id": "Root=1-60f9e64c-36256b4878dc640c5af75f7d",
        "x-forwarded-port": "443",
        "x-forwarded-proto": "https"
    },
    "json": null,
    "url": "https://postman-echo.com/post"
}
```