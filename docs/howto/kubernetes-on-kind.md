# Running Kubernetes on Kubernetes in Docker (KinD)

This tutorial will focus on using a tool called [kind](https://github.com/kubernetes-sigs/kind),
also known as "Kubernetes IN Docker".

If you haven't installed them already, go ahead and [install
Docker](https://docs.docker.com/install/), [install
kind](https://github.com/kubernetes-sigs/kind#installation-and-usage), and [install
kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl/).

You'll need `kubectl` to interact with the cluster once it's created.

## Create a cluster

Once Docker, kind, and kubectl are installed, create a cluster with kind:

```console
$ kind create cluster
```

This will create a cluster with a single node - perfect for local development.

You should see output similar to the following:

```console
Creating cluster "kind" ...
 ✓ Ensuring node image (kindest/node:v1.17.0) 🖼
 ✓ Preparing nodes 📦
 ✓ Writing configuration 📜
 ✓ Starting control-plane 🕹️
 ✓ Installing CNI 🔌
 ✓ Installing StorageClass 💾
Set kubectl context to "kind-kind"
You can now use your cluster with:

kubectl cluster-info --context kind-kind

Have a nice day! 👋
```

Now we can interact with our cluster! Try that out now:

```console
$ kubectl cluster-info
Kubernetes master is running at https://127.0.0.1:32768
KubeDNS is running at https://127.0.0.1:32768/api/v1/namespaces/kube-system/services/kube-dns:dns/proxy
```

To further debug and diagnose cluster problems, use 'kubectl cluster-info dump'.
