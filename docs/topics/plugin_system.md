# Plugin System Overview

Krustlet partially implements the plugin discovery system used by the mainline
Kubelet for purposes of supporting CSI. The CSI documentation points at the
[device plugin documentation](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/#device-plugin-registration),
but upon further investigation/reverse engineering, we determined that CSI
plugins use the auto plugin discovery method implemented
[here](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/pluginmanager).
You can also see other evidence of this in the
[csi-common code](https://github.com/kubernetes-csi/drivers/blob/master/pkg/csi-common/nodeserver-default.go)
and the [Node Driver Registrar
documentation](https://github.com/kubernetes-csi/node-driver-registrar/blob/be7678e75e23b5419624ae3983b66957c0991073/README.md).

## What is not supported?

Currently we do not support the `DevicePlugin` type or the aforementioned newer
device plugin system. Currently we do not have plans to implement it, but that
could change in the future as needs/uses evolve

## How does it work?

The plugin registration system has an event driven loop for discovering and
registering plugins:

1. Kubelet using a file system watcher to watch the given directory
2. Plugins wishing to register themselves with Kubelet must open a Unix domain
   socket (henceforth referred to as just "socket") in the watched directory
3. When Kubelet detects a new socket, it connects to the discovered socket and
   attempts to do a `GetInfo` gRPC call.
4. Using the info returned from the `GetInfo` call, Kubelet performs validation
   to make sure it supports the correct version of the API requested by the
   plugin and that the plugin is not already registered. If it is a `CSIPlugin`
   type, the info will also contain another path to a socket where the CSI
   driver is listening
5. If validation succeeds, Kubelet makes a `NotifyRegistrationStatus` gRPC call
   on the originally discovered socket to inform the plugin that it has
   successfully registered

### Additional information

In normal Kubernetes land, most CSI plugins register themselves with the Kubelet
using the [Node Driver
Registrar](https://github.com/kubernetes-csi/node-driver-registrar) sidecar
container that runs with the actual CSI driver. It has the responsibilty for
creating the socket that Kubelet discovers.
