# Providers

There are two different runtimes available for Krustlet: `wascc` or `wasi`.

The `wascc` runtime is a secure WebAssembly host runtime, connecting "actors" and "capability providers" together to
connect your WebAssembly runtime to cloud-native services like message brokers, databases, or other external services
normally unavailable to the WebAssembly runtime.

The `wasi` runtime uses a project called [`wasmtime`](https://github.com/bytecodealliance/wasmtime). wasmtime is a
standalone JIT-style host runtime for WebAssembly modules. It is focused primarily on standards compliance with the WASM
specification as it relates to [WASI](https://wasi.dev/). If your WebAssembly module complies with the
[WebAssembly specification](https://github.com/WebAssembly/spec), wasmtime can run it.

It's important to note that the WASI standard and `wasmtime` are still under heavy development. There are some key
features (like networking) that are currently missing, but will be made available in future updates.
