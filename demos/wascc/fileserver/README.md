# Fileserver

An example that will respond with the file metadata present in the volume, based on the URL path
provided.

If a POST request is made, the contents of the request body are written to the file based on the URL
path provided.

If a DELETE request is made, the file is removed based on the URL path provided.

It is meant to demonstrate how volumes work with the wascc-provider.

## Running the example

This example has already been pre-built, so you only need to install it into your Kubernetes
cluster.

Create the pod and configmap with `kubectl`:

```shell
$ kubectl create -f k8s.yaml
```

## Building the example

To set up your development environment, you'll need the following tools:

- cargo
- wasm-to-oci
- wascap
- nk

Instructions for [installing
`cargo`](https://doc.rust-lang.org/cargo/getting-started/installation.html) and
[`wasm-to-oci`](https://github.com/engineerd/wasm-to-oci) can be found in their respective project's
documentation. Once those are installed, [`wascap`](https://crates.io/crates/wascap) and [`nkeys`](https://crates.io/crates/nkeys) can be installed with

```
cargo install wascap --features "cli"
cargo install nkeys --features "cli"
```

Once complete, run `make` to compile the example.
