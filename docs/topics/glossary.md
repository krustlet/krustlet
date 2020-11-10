# Glossary

Here is where you will find definitions for commmon terminology used across
Krustlet.

## Kubelet

The kubelet is a key piece of the Kubernetes architecture. A kubelet is a "node
agent" that runs on each node in a Kubernetes cluster. It registers as a node
with the Kubernetes API, waiting for new [pods](#pod) provided by the API, and
ensures the workloads in those pods are running and healthy.

## Pod

A pod is the simplest execution unit in Kubernetes that you can create and
destroy using the Kubernetes API. Nearly every workload type available in
Kubernetes (Deployments, StatfulSets, DaemonSets, Jobs, etc.) uses pods as the
basic unit of work. In other words, a pod represents a single unit of work in
your cluster.

## Provider

A provider is an abstract interface within Krustlet. Providers describe the
verbs and actions a WebAssembly runtime (like wasmtime) must provide in order
for that runtime to work as a kubelet.

The primary responsibility of a provider is to execute a workload (or schedule
it on an external executor), monitor that workload, and expose important details
back to Kubernetes using the Kubelet API.

See also the topic guide on [providers](providers.md).
