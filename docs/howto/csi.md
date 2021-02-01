# Registering a CSI Driver

For more information on what is the Container Storage Interface and how it
relates to a CSI driver, see the [topic guide](../topics/csi.md) for more
information.

A Krustlet Provider with CSI support will check for new drivers registered to
the `plugins/` directory (by default, this is `$HOME/.krustlet/plugins`). You
will need to inform your CSI driver to bind its socket at that location in order
for a Provider to recognize and register the driver.

You will also need to install and run the following projects so that the
PersistentVolumeClaim's volume will be provisioned and readily available for
use:

- [node-driver-registrar](https://github.com/kubernetes-csi/node-driver-registrar)
- [external-provisioner](https://github.com/kubernetes-csi/external-provisioner)

Do keep in mind that some CSI drivers rely on linux-specific command line
tooling like `mount`, so these tools may only work on Linux. Cross-platform
support is not guaranteed. Refer to the driver's documentation for more
information.

## How do I use a CSI Volume?

Assuming a CSI storage plugin is already deployed on your cluster, you can use
it through familiar Kubernetes storage primitives: PersistentVolumeClaims,
PersistentVolumes, and StorageClasses.

A basic example to start with would be the [host-path CSI
driver](https://github.com/kubernetes-csi/csi-driver-host-path). This project is
not recommended for use in production, but will work as an example.

To start, we'll need to create a StorageClass. A StorageClass provides a way for
administrators to describe the "classes" of storage they offer. Different
classes might map to quality-of-service levels, or to backup policies, or to
arbitrary policies determined by the cluster administrators. Kubernetes itself
is unopinionated about what classes represent. This concept is sometimes called
"profiles" in other storage systems.

The following StorageClass enables dynamic creation of "csi-hostpath-sc" volumes
by a CSI volume plugin called "hostpath.csi.k8s.io". This storage class will
also allow for [volume
expansion](https://github.com/kubernetes-csi/external-resizer).

```yaml
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: csi-hostpath-sc
provisioner: hostpath.csi.k8s.io
reclaimPolicy: Delete
volumeBindingMode: Immediate
allowVolumeExpansion: true
```

After installing a StorageClass, we can start creating PersistentVolumeClaims to
provision and prepare volumes. These will be mounted to Kubernetes Pods that
request it as a volume. Note how the PVC requests the same StorageClass we
created earlier.

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: csi-pvc
spec:
  accessModes:
  - ReadWriteOnce
  resources:
    requests:
      storage: 1Gi
  storageClassName: csi-hostpath-sc
```

A Pod can then request that volume to be mounted by using the `volumes` API.

```yaml
kind: Pod
apiVersion: v1
metadata:
  name: my-frontend
spec:
  containers:
    - name: my-frontend
      image: example.com/my-frontend:v1.0.0
      volumeMounts:
      - mountPath: "/data"
        name: my-csi-volume
  volumes:
    - name: my-csi-volume
      persistentVolumeClaim:
        claimName: csi-pvc
```

When the pod referencing a CSI volume is scheduled, Krustlet will trigger the
appropriate operations against the external CSI plugin (ControllerPublishVolume,
NodePublishVolume, etc.) to ensure the specified volume is attached, mounted,
and ready to use by the containers in the pod.

## Addendum: Role-based Access Control

In the event that the Pod is erroring saying that the kubelet doesn't have the
correct admission controls to access storage classes, you'll have to create the
following cluster role and role binding to allow the `system:nodes` group to
access them:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: storageclass-reader
rules:
  - apiGroups: ["storage.k8s.io"]
    resources: ["storageclasses"]
    verbs: ["get", "watch", "list"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: node-storageclass-reader
subjects:
  - kind: Group
    name: system:nodes
    apiGroup: rbac.authorization.k8s.io
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: storageclass-reader
```
