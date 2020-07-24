# Running Web Assembly (WASM) workloads in Kubernetes

The Krustlet repository contains two projects, `krustlet-wasi` and
`krustlet-wascc`, for running WASM workloads in Kubernetes. These kubelets
run workloads implemented as Web Assembly (WASM) modules rather than
as OCI containers. Each running instance appears to Kubernetes as a
node; Kubernetes schedules work to the instance as to any other node.

## Running WASM modules on the right kubelet

WASM modules are not interchangeable with OCI containers: `krustlet-wasi`
and `krustlet-wascc` can't run OCI containers, and normal OCI nodes
can't run WASM modules. In order to run your WASM workloads on the right
nodes, you should use the Kubernetes tolerations system; in some cases you
will also need to use node affinity.

The `krustlet-wasi` and `krustlet-wascc` 'virtual nodes' both have
`NoExecute` taints with the key `krustlet/arch` and a provider-defined
value (`wasm32-wasi` or `wasm32-wascc` respectively).  WASM pods must
therefore specify a toleration for this taint.  For example:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: hello-wasm
spec:
  containers:
  - name: hello-wasm
    image: webassembly.azurecr.io/hello-wasm:v1
  tolerations:
  - effect: NoExecute
    key: krustlet/arch
    operator: Equal
    value: wasm32-wasi   # or wasm32-wascc according to module target arch
```

In addition, if the Kubernetes cluster contains 'standard' OCI nodes which
do not taint themselves, you should prevent Kubernetes from scheduling
WASM workloads to those nodes.  To do this, you can either taint the OCI
nodes (though this may require you to provide suitable tolerations on
OCI pods), or you can specify a node selector on the WASM workload to
direct it to compatible nodes:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: hello-wasm
spec:
  # other values as above
  nodeSelector:
    kubernetes.io/arch: wasm32-wasi  # or wasm32-wascc
```

If you get intermittent image pull errors on your WASM workloads, check
that they are not inadvertently getting scheduled to OCI nodes.
