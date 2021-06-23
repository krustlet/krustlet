# Plugin System Overview

Krustlet partially implements support for CSI and device plugins. For CSI
plugins support, Krustlet partially implements the plugin discovery system used
by the mainline Kubelet. Upon investigation/ reverse engineering, we determined
that CSI and device plugins use different APIs, the [plugin
registration](../../crates/kubelet/proto/pluginregistration/v1/pluginregistration.proto)
and [device
plugin](../../crates/kubelet/proto/deviceplugin/v1beta1/deviceplugin.proto)
APIs, respectively. CSI plugins use the auto plugin discovery method implemented
[here](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/pluginmanager).
You can see other evidence of this in the [csi-common
code](https://github.com/kubernetes-csi/drivers/blob/master/pkg/csi-common/nodeserver-default.go)
and the [Node Driver Registrar
documentation](https://github.com/kubernetes-csi/node-driver-registrar/blob/be7678e75e23b5419624ae3983b66957c0991073/README.md).
Instead of watching for plugins as done by the CSI [`pluginwatcher`
package](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/pluginmanager/pluginwatcher)
in the kubelet, the kubelet
[`devicemanager`](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/cm/devicemanager)
hosts a registration service for device plugins, as described in the [device
plugin
documentation](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/#device-plugin-registration).


## CSI Plugins
### Registration: How does it work?

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

In normal Kubernetes land, most CSI plugins register themselves with the kubelet
using the [Node Driver
Registrar](https://github.com/kubernetes-csi/node-driver-registrar) sidecar
container that runs with the actual CSI driver. It has the responsibility for
creating the socket that the kubelet discovers.

## Device Plugins

Krustlet supports Kubernetes [device
plugins](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/),
which enable Kubernetes workloads to request extended resources, such as
hardware, advertised by device plugins. Krustlet implements the [Kubernetes
device
manager](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/cm/devicemanager)
in the kubelet's [`resources` module](../../crates/kubelet/src/resources). It
implements the [device plugin framework's `Registration` gRPC
service](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/#device-plugin-registration).


Flow from registering device plugin  (DP) to running a Pod requesting an DP
extended resource:
1. The kubelet's `DeviceManager` hosts the [device plugin framework's
   `Registration` gRPC
   service](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/#device-plugin-registration)
   on the Kubernetes default `/var/lib/kubelet/device-plugins/kubelet.sock`. 
1. DP registers itself with the kubelet through this gRPC service. This allows
   the DP to advertise a resource such as system hardware to kubelet. 
1. The kubelet creates a `PluginConnection` for marshalling requests to the DP.
   It calls the DP's `ListAndWatch` service, creating a bi-directional streaming
   connection. The device plugin updates the kubelet about the device health
   across this connection. 
1. Each time the `PluginConnection` receives device updates across the
   `ListAndWatch` connection. It updates the map of all devices (`DeviceMap`)
   shared between the `DeviceManager`, `PluginConnections` and `NodePatcher` and
   notifies the `NodePatcher` to update the `NodeStatus` of the node with
   appropriate `allocatable` and `capacity` entries.
1. Once a Pod is applied that requests the resource advertized by the DP (say
   `example.com/mock-plugin`). Then the K8s scheduler can schedule the Pod to
   this node, since the requested resource is `allocatable` in the `NodeSpec`.
   During the `Resources` state, if a DP resource is requested, the
   `PluginConnection` calls `Allocate` on the DP, requesting use of the
   resource. 
1. If the Pod is terminated, in order to free up the DP resource, the
   `DeviceManager` contains a `PodDevices` structure that queries K8s Api for
   currently running Pods before each allocate call. It then will update it's
   map of allocated devices to remove terminated Pods.
1. If the DP dies and the connection is dropped, the devices are removed from
   the `DeviceMap` and the `NodePatcher` zeros `capacity` and `allocatable` for
   the resource in the NodeSpec.

### What is not supported?
The current implementation does not support the following:
1. Calls to a device plugin's `GetPreferredAllocation` endpoint in order to make
   more informed `Allocate` calls.
2. Each
   [`ContainerAllocateResponse`](../../crates/kubelet/proto/deviceplugin/v1beta1/deviceplugin.proto#L181)
   contains environment variables, mounts, device specs, and annotations that
   should be set in Pods that request the resource. Currently, Krustlet only
   supports a subset of `Mounts`, namely `host_path` mounts as volumes. 
1. Does not consider
   [`Device::TopologyInfo`](../../crates/kubelet/proto/deviceplugin/v1beta1/deviceplugin.proto#L98),
   as the [Topology
   Manager](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/#device-plugin-integration-with-the-topology-manager)
   has not been implemented in Krustlet.