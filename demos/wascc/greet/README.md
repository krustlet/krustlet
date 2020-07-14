# Greet

An example that will greet the user with an HTTP response.

It is meant to be a simple demo for the wascc-provider with Krustlet.

## Running the example

This example has already been pre-built, so you only need to install it into your Kubernetes
cluster.

Create the pod and configmap with `kubectl`:

```shell
$ kubectl apply -f greet-wascc.yaml
```

If the container port is specified in the yaml file, but host port is not. A random port will be assigned. Look for **New port assigned is: xxxxx"** in the logs. Then, run **curl localhost:xxxxx** with the assigned port number.
To assign a specific host port, add **hostPort: xxxxx** in the yaml files in a new line under containerPort: 8080
