# Providers

The default runtime for the Krustlet project is `wasi`.

The `wasi` runtime uses a project called
[`wasmtime`](https://github.com/bytecodealliance/wasmtime). wasmtime is a
standalone JIT-style host runtime for WebAssembly modules. It is focused
primarily on standards compliance with the WASM specification as it relates to
[WASI](https://wasi.dev/). If your WebAssembly module complies with the
[WebAssembly specification](https://github.com/WebAssembly/spec), wasmtime can
run it.

It's important to note that the WASI standard and `wasmtime` are still under
heavy development. There are some key features (like networking) that are
currently missing, but will be made available in future updates.

## Additional Providers

There are various other providers available as well.

- [`wasmcloud`](https://github.com/wasmCloud/krustlet-wasmcloud-provider): The
  `wasmcloud` runtime is a secure WebAssembly host runtime, connecting "actors"
  and "capability providers" together to connect your WebAssembly runtime to
  cloud-native services like message brokers, databases, or other external
  services normally unavailable to the WebAssembly runtime. This provider used
  to be available in this repo but was moved under the wasmCloud project so it
  could be maintained both by the Krustlet maintainers and the wasmCloud
  maintainers.
- [`CRI`](https://github.com/kflansburg/krustlet-cri): A Container Runtime
  Interface provider implementation for Krustlet. This runtime allows you to run
  the containers you know and love within Krustlet.
