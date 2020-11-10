# Introduction

Krustlet is a tool to run WebAssembly workloads natively on Kubernetes. Krustlet
acts like a node in your Kubernetes cluster. When a user schedules a Pod with
certain node tolerations, the Kubernetes API will schedule that workload to a
Krustlet node, which will then fetch the module and run it.

Krustlet implements the [kubelet](../topics/glossary.md#kubelet) API, and it
will respond to common API requests like `kubectl logs` or `kubectl delete`.

If you'd like to learn how to deploy Krustlet on your own cluster (or if you're
just getting started), follow the [quickstart guide](quickstart.md) for
instructions on deploying your first Kubernetes cluster.

In order for your application to run on a Kruslet node, the application must be
compiled to WebAssembly and pushed to a container registry. If you'd like to
learn more about how to write your own WebAssembly module in Rust and deploy it,
[follow through the tutorial](tutorial01.md) to deploy your first application.
