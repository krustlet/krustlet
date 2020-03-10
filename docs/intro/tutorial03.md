# Writing your first app, part 3

This tutorial begins where [Tutorial 2](tutorial02.md) left off. We’ll walk through the process for running a Kubernetes
cluster on your local computer. We'll also learn how to install Krustlet on this cluster so you can install and run your
application.

## Prerequisites

This tutorial will focus on using a tool called [kind](https://github.com/kubernetes-sigs/kind), also known as
"Kubernetes IN Docker".

If you haven't installed them already, go ahead and [install Docker](https://docs.docker.com/install/),
[install kind](https://github.com/kubernetes-sigs/kind#installation-and-usage), and [install kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl/).

You'll need `kubectl` to interact with the cluster once it's created.

## Create a cluster

Once Docker, kind, and kubectl are installed, create a cluster with kind:

```console
$ kind create cluster
```

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

To further debug and diagnose cluster problems, use 'kubectl cluster-info dump'.
```

## Install Krustlet

Krustlet can start register itself with the Kubernetes cluster even if it's running locally on your computer.

There are two different runtimes available for Krustlet: `wascc` or `wasi`.

The `wascc` runtime is a secure WebAssembly host runtime, connecting "actors" and "capability providers" together to
connect your WebAssembly runtime to cloud-native services like message brokers, databases, or other external services
normally unavailable to the WebAssembly runtime.

The `wasi` runtime uses a project called [`wasmtime`](https://github.com/bytecodealliance/wasmtime). wasmtime is a
standalone JIT-style host runtime for WebAssembly modules. It is focused primarily on standards compliance with the WASM
specification as it relates to [WASI](https://wasi.dev/). If your WebAssembly module complies with the
[WebAssembly specification](https://github.com/WebAssembly/spec), wasmtime can run it.

Our "hello world" application does not require any connection to external services, so the `wasi` runtime will work just
fine for our use case.

Since we want to interact with the Krustlet (for things like `kubectl logs` and `kubectl exec`), we'll need to tell
Kubernetes what IP address Krustlet is listening on. Otherwise, certain API calls will result in errors.

To set the node IP, run:

```console
$ export KRUSTLET_NODE_IP=<your ip address>
```

Now that we're ready, let's run it!

```console
$ just run-wasi
```

Eventually, you should see a log message in your terminal window:

```console
   Compiling krustlet v0.1.0 (/home/bacongobbler/code/krustlet)
    Finished dev [unoptimized + debuginfo] target(s) in 1m 39s
     Running `/home/bacongobbler/code/krustlet/target/debug/krustlet-wasi`
```

Keep this terminal window running; you'll need to keep Krustlet running if you want to run your application.

After you've finished installing Krustlet, read [part 4 of this tutorial](tutorial04.md) to install your application.
