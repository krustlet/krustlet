# Hello World AssemblyScript for wasCC

A simple hello world example in AssemblyScript that will print the environment variables available
to the process as an HTTP response.

It is meant to be a simple demo for the wascc-provider with Krustlet.

## Running the example

This example has already been pre-built, so you only need to install it into your Kubernetes
cluster.

Create the pod and configmap with `kubectl`:

```shell
$ kubectl apply -f k8s.yaml
```

If the container port is specified in the yaml file, but host port is not. A random port will be assigned. Look for **New port assigned is: xxxxx"** in the logs. Then, run **curl localhost:xxxxx** with the assigned port number.
To assign a specific host port, add **hostPort: xxxxx** in the yaml files in a new line under containerPort: 8080

## Building from Source

If you want to compile the demo and inspect it, you'll need to do the following.

### Prerequisites

You'll need `npm` installed in order to install and build the dependencies. This project is using
the [as-wasi](https://github.com/jedisct1/as-wasi) dependency, which is a helpful set of wrappers
around the low level WASI bindings provided in AssemblyScript.

You'll also need
[`wapc-gql2as`](https://github.com/wapc/as-codegen/tree/feature/initial_implementation). You'll have
to compile it from source.

If you are interested in starting your own AssemblyScript project, visit the AssemblyScript [getting
started guide](https://docs.assemblyscript.org/quick-start).

If you don't have Krustlet with the WASI provider running locally, see the instructions in the
[tutorial](../../../docs/intro/tutorial03.md) for running locally.

### Compiling

Run:

```shell
$ npm install
$ npm run codegen
$ npm run asbuild
```

### Pushing

Before pushing the actor module, you will need to sign it and grant it the `wascc:http_server`
capability. See [the waSCC documentation](https://wascc.dev/tutorials/first-actor/sign_module/) for
more information.

```
$ wascap sign build/optimized.wasm build/optimized_signed.wasm -u module.nk -i account.nk -n "Hello World" -s
```

Once it's signed, detailed instructions for pushing a module can be found in the
[tutorial](../../../docs/intro/tutorial02.md).
