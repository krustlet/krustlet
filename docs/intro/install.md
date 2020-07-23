# Install Krustlet

This guide shows how to install Krustlet.

## From the Binary Releases

Every release of Krustlet provides compiled releases for a variety of Operating Systems. These
compiled releases can be manually downloaded and installed. Please note these instructions will work
on Linux, MacOS, and Windows (in PowerShell)

1. Download your desired version from [the releases
   page](https://github.com/deislabs/krustlet/releases)
1. Unpack it (`tar -xzf krustlet-v0.3.0-linux-amd64.tar.gz`)
1. Find the desired Krustlet provider in the unpacked directory, and move it to its desired
   destination somewhere in your `$PATH` (e.g. `mv krustlet-wasi /usr/local/bin/` on unix-like
   systems or `mv krustlet-wasi.exe C:\Windows\system32\` on Windows)

From there, you should be able to run the client in your terminal emulator. If your terminal cannot
find Krustlet, check to make sure that your `$PATH` environment variable is set correctly.

### Validating

If you'd like to validate the download, checksums can be downloaded from
https://krustlet.blob.core.windows.net/releases/checksums-v0.3.0.txt

### Windows

As of Krustlet 0.4, there are now Windows builds available. However, there are some caveats. The
underlying dependencies used to support Windows do not support certs with IP SANs (subject alternate
names). Because of this, the serving certs requested during bootstrap will not work for local
development options like minikube or KinD as they do not have an FQDN. So these builds can only be
used in environments with an actual hostname/FQDN accessible to the Kubernetes cluster.

## From Canary Builds

“Canary” builds are versions of Krustlet that are built from `master`. They are not official
releases, and may not be stable. However, they offer the opportunity to test the cutting edge
features before they are released.

Here are links to the common builds:

- [checksum file](https://krustlet.blob.core.windows.net/releases/checksums-canary.txt)
- [64-bit Linux (AMD
  architecture)](https://krustlet.blob.core.windows.net/releases/krustlet-canary-linux-amd64.tar.gz)
- [64-bit macOS (AMD
  architecture)](https://krustlet.blob.core.windows.net/releases/krustlet-canary-macos-amd64.tar.gz)
- [64-bit Windows](https://krustlet.blob.core.windows.net/releases/krustlet-canary-windows-amd64.tar.gz)

## Compiling from Source

If you want to compile Krustlet from source, you will need to follow the [developer
guide](../community/developers.md).

## Next Steps

After installing Krustlet, if you'd like to get started and see something running, go checkout any
one of the [demos](../../demos). Each of them has a prebuilt WebAssembly module stored in a registry
and a Kubernetes manifest that you can `kubectl apply`.

If you'd like to learn how to write your own simple module in Rust and deploy it, [follow through
the tutorial](tutorial01.md) to deploy your first application.
