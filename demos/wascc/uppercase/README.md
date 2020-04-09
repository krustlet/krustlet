# Uppercase

An example that will respond with the uppercased version of the querystring sent in.

It is meant to be a simple demo for the wascc-provider with Krustlet.

## Video

You can watch a video of the creation of this actor on [Youtube](https://www.youtube.com/watch?v=uy91W7OxHcQ).

## Running the example

This example has already been pre-built, so you only need to install it into your Kubernetes
cluster.

Create the pod and configmap with `kubectl`:

```shell
$ kubectl apply -f uppercase-wascc.yaml
```
