# Running Krustlet on Minikube

This how-to guide demonstrates how to boot a Krustlet node in a Minikube cluster.

## Prerequisites

You will require a running Minikube cluster for this how-to. The steps below assume that minikube
was booted with the VirtualBox driver, though other drivers can be used with some changes. `kubectl`
is also required.

See the [how-to guide for running Kubernetes on Minikube](kubernetes-on-minikube.md) for more
information.

This specific tutorial will be running Krustlet on your host Operating System; however, you can
follow these steps from any device that can start a web server on an IP accessible from the
Kubernetes control plane, including Minikube itself.

## Step 1: Get a bootstrap config

Krustlet requires a bootstrap token and config the first time it runs. Follow the guide
[here](bootstrapping.md) to generate a bootstrap config and then return to this document. This will
If you already have a kubeconfig available that you generated through another process, you can
proceed to the next step. However, the credentials Krustlet uses must be part of the `system:nodes`
group in order for things to function properly.

## Step 2: Determine the default gateway

The default gateway when you [set up minikube with the VirtualBox driver](kubernetes-on-minikube.md)
is generally `10.0.2.2`. We can use this IP address from the guest Operating System (the minikube
host) to connect to the host Operating System (where Krustlet is running). If this was changed, use
`minikube ssh` and `ip addr show` from the guest OS to determine the default gateway.

## Step 3: Install and run Krustlet

First, install the latest release of Krustlet following [the install guide](../intro/install.md).

Once you have done that, run the following commands to run Krustlet's WASI provider:

```shell
# Since you are running locally, this step is important. Otherwise krustlet will pick up on your
# local config and not be able to update the node status properly
$ export KUBECONFIG=~/.krustlet/config/kubeconfig
$ krustlet-wasi --node-ip 10.0.2.2 --cert-file=~/.krustlet/config/krustlet.crt --private-key-file=~/.krustlet/config/krustlet.key --bootstrap-file=~/.krustlet/config/bootstrap.conf
```

### Step 3a: Approving the serving CSR

Once you have started Krustlet, there is one more manual step (though this could be automated
depending on your setup) to perform. The client certs Krustlet needs are generally approved
automatically by the API. However, the serving certs require manual approval. To do this, you'll
need the hostname you specified for the `--hostname` flag or the output of `hostname` if you didn't
specify anything. From another terminal that's configured to access the cluster, run:

```bash
$ kubectl certificate approve <hostname>-tls
```

NOTE: You will only need to do this approval step the first time Krustlet starts. It will generate
and save all of the needed credentials to your machine

In another terminal, run `kubectl get nodes -o wide` and you should see output that looks similar to
below:

```
NAME       STATUS   ROLES    AGE   VERSION   INTERNAL-IP      EXTERNAL-IP   OS-IMAGE               KERNEL-VERSION   CONTAINER-RUNTIME
minikube   Ready    master   18m   v1.18.0   192.168.99.165   <none>        Buildroot 2019.02.10   4.19.107         docker://19.3.8
krustlet   Ready    agent    9s    v1.17.0   10.0.2.2         <none>        <unknown>              <unknown>        mvp
```
