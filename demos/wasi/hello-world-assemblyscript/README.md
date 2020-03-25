# Hello World AssemblyScript for WASI
A simple hello world example in AssemblyScript that will print:
- The environment variables available to the process
- Text to both stdout and stderr.
- Any args passed to the process 

It is meant to be a simple demo for the wasi-provider with Krustlet.

## Prerequisites
You'll need `npm` installed in order to install and build the dependencies. This
project is using the [as-wasi](https://github.com/jedisct1/as-wasi) dependency,
which is a helpful set of wrappers around the low level wasi bindings provided
in AssemblyScript. If you are interested in starting your own AssemblyScript
project, visit the AssemblyScript [getting
started](https://docs.assemblyscript.org/quick-start) guide.

If you don't have Krustlet with the WASI provider running locally, see the
instructions in the [tutorial](../../../docs/intro/tutorial03.md) for running
locally.

## Building
Simply run:

```shell
$ npm install && npm run asbuild
```

## Pushing
Detailed instructions for pushing a module can be found
[here](../../../docs/intro/tutorial02.md). We hope to improve and streamline the
build and push process in the future. However, for test purposes, the image has
been pushed to the `webassembly` Azure Container Registry

## Running the example
First create the pod and configmap:

```shell
$ kubectl apply -f k8s.yaml
```

You should then be able to get the logs and see the output from the wasm module
run:

```shell
$ kubectl logs hello-world-wasi-assemblyscript                                                                                   
hello from stdout!
hello from stderr!
FOO=bar
CONFIG_MAP_VAL=cool stuff
POD_NAME=hello-world-wasi-assemblyscript
Args are: []
```
