# Greet

An example that will display a greeting on standard output.

It is meant to be a simple demo for the wasi-provider with Krustlet.

## Running the example

This example has already been pre-built, so you only need to install it into
your Kubernetes cluster.

Create the pod and configmap with `kubectl`:

```shell
$ kubectl apply -f greet-wasi.yaml
```

Check the logs to see the greeting:

```shell
$ kubectl logs greet
```
