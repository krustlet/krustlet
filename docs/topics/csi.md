# The Container Storage Interface

The Container Storage Interface (CSI) is a standardized plugin system that
enables many different types of storage systems to

1. Automatically provision storage volumes as needed
1. Mount volumes to pods as needed
1. Unmount volumes from deleted or removed pods, and
1. Destroy storage volumes after they've been de-commissioned.

Krustlet introduced this feature in v0.6.0 and is currently in alpha status.
Many features that Kubernetes supports such as "Block" mounting or read-only
access modes are currently unavailable, but will become available as the feature
stabilizes.

## Why CSI?

Without CSI support, adding a new storage system to a Provider requires checking
code into the core Krustlet repository, and it requires each Provider to call
this code in order to support the new volume type. Any changes to the storage
system won't become available until the next Krustlet release, and could be
painful for many Providers to adopt these new changes.

CSI addresses these issues by enabling storage plugins to be developed
out-of-tree, deployed alongside a Krustlet Provider, and consumed through
standard Kubernetes storage primitives: PersistentVolumeClaims (PVC),
PersistentVolumes (PV), and StorageClasses (SC).

## How do I deploy a CSI driver alongside a Krustlet Provider?

Please see the [HOWTO guide](../howto/csi.md) for more information.

## How do I use a CSI Volume?

Please see the [HOWTO guide](../howto/csi.md) for more information.

## Where can I find CSI Drivers?

CSI drivers are maintained and distributed by the community. You can find
example CSI drivers in the [kubernetes-csi](https://github.com/kubernetes-csi)
organization on GitHub. These are provided purely for illustrative purposes, and
are not intended for use in production.
