# Krustlet architecture

This document describes the Krustlet architecture at a high level.

## The purpose of Krustlet

Krustlet acts as a Kubernetes Kubelet by listening on the Kubernetes API's event stream for new Pod requests that match
a particular set of node selectors, scheduling those workloads to run using a WASI-based runtime instead of a
container-based runtime.

## Implementation

Krustlet is written in Rust.

By acting as a Kubelet, Krustlet uses Kubernetes client libraries to communicate with the Kubernetes API. Currently,
the client libraries use HTTP(s) as the communication protocol between Krustlet and the Kubernetes API, using JSON as
the data format for serializing and de-serializing request bodies.

Krustlet sends status updates about scheduled pods to the Kubernetes API. Therefore, it does not require its own
database.
