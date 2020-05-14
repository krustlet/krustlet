# Hello World Golang for WASI

A simple hello world example in Golang that will print:

- The environment variables available to the process
- Text to stdout and stderr
-- Any args passed to the process

It is meant to be a simple demo for the wasi-provider with Krustlet.

## Running the example

First create the pod and configmap:

```shell
$ kubectl apply --filename=k8s.yaml
```

You should then be able to get the logs and see the output from the wasm module run:

```shell
$ kubectl logs pod/hello-world-wasi-golang
hello from stdout!
hello from stderr!
FOO=bar
CONFIG_MAP_VAL=cool stuff
POD_NAME=hello-world-wasi-golang
Args are: [/main arg1 arg2]
```

## Building from Source

If you want to compile the demo and inspect it, you'll need to do the following.

### Prerequisites

You'll need to have Golang installed.

### Running

Run:

```shell
$ go run . arg1 arg2
```

### Building

Run:

```shell
$ GOOS=js GOARCH=wasm go build # -o hello-world-wasi-golang.wasm
```

### Pushing

Detailed instructions for pushing a module can be found [here](../../../docs/intro/tutorial02.md).