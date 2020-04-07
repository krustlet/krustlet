# Install Krustlet

This guide shows how to install Krustlet.

## From the Binary Releases

Every release of Krustlet provides compiled releases for a variety of Operating Systems. These
compiled releases can be manually downloaded and installed.

1. Download your desired version from [the releases
   page](https://github.com/deislabs/krustlet/releases)
1. Unpack it (`tar -xzf krustlet-v0.1.0-Linux-amd64.tar.gz`)
1. Find the desired Krustlet provider in the unpacked directory, and move it to its desired
   destination (`mv krustlet-wasi /usr/local/bin/`)

From there, you should be able to run the client in your terminal emulator. If your terminal cannot
find Krustlet, check to make sure that your `$PATH` environment variable is set correctly.

### Special instructions for `krustlet-wascc`
In version 0.1, `krustlet-wascc` requires some precompiled object files in order to run its HTTP
server and logging. These steps are required for version 0.1 but will not be required in version
0.2.

In the unpacked directory, you should see several `.so` and `.dylib` folders. These should be placed
in `$HOME/.krustlet/lib` (or in `$KRUSTLET_DATA_DIR/lib` if you set a custom `KRUSTLET_DATA_DIR`)
like so:

```bash
$ mv lib/*.{so,dylib} $HOME/.krustlet/lib/
```

## From Canary Builds

“Canary” builds are versions of Krustlet that are built from `master`. They are not official
releases, and may not be stable. However, they offer the opportunity to test the cutting edge
features before they are released.

Here are links to the common builds:

- [checksum file](https://krustlet.blob.core.windows.net/releases/checksums-canary.txt)
- [64-bit Linux (AMD
  architecture)](https://krustlet.blob.core.windows.net/releases/krustlet-canary-Linux-amd64.tar.gz)
- [64-bit macOS (AMD
  architecture)](https://krustlet.blob.core.windows.net/releases/krustlet-canary-macOS-amd64.tar.gz)

## Compiling from Source

If you want to compile Krustlet from source, you will need to follow the [developer
guide](../community/developers.md).

## Next Steps

After installing Krustlet, if you'd like to get started and see something running, go checkout any
one of the [demos](../../demos). Each of them has a prebuilt WebAssembly module stored in a registry
and a Kubernetes manifest that you can `kubectl apply`.

If you'd like to learn how to write your own simple module in Rust and deploy it, [follow through
the tutorial](tutorial01.md) to deploy your first application.
