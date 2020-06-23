# Running Krustlet on Kubernetes in Docker (KinD)

This how-to guide demonstrates how to boot a Krustlet node in a KinD cluster.

## Prerequisites

You will require a running KinD cluster for this how-to. `kubectl` is also required. See the [how-to
guide for running Kubernetes on KinD](kubernetes-on-kind.md) for more information.

This specific tutorial will be running Krustlet on your host Operating System; however, you can
follow these steps from any device that can start a web server on an IP accessible from the
Kubernetes control plane, including KinD itself.


## Step 1: Get a bootstrap config

Krustlet requires a bootstrap token and config the first time it runs. Follow the guide
[here](bootstrapping.md) to generate a bootstrap config and then return to this document.
If you already have a kubeconfig available that you generated through another process, you can
proceed to the next step. However, the credentials Krustlet uses must be part of the `system:nodes`
group in order for things to function properly.

## Step 2: Determine the default gateway

The default gateway for most Docker containers (including your KinD host) is generally `172.17.0.1`.
We can use this IP address from the guest Operating System (the KinD host) to connect to the host
Operating System (where Krustlet is running). If this was changed, check `ip addr show docker0` from
the host OS to determine the default gateway.

### Special note: Docker Desktop for Mac

For Docker Desktop for Mac users, [the `docker0` bridge network is unreachable from the host
network](https://docs.docker.com/docker-for-mac/networking/#use-cases-and-workarounds) (and vice
versa). However, the `en0` host network is accessible from within the container.

Because the `en0` network is the default network, Krustlet will bind to this IP address
automatically. You should not need to pass a `--node-ip` flag to Krustlet.

In the event this does not appear to be the case (for example, when the hostname cannot resolve to
this address), check which IP address you have for the `en0` network:

```console
$ ifconfig en0
en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500
        options=400<CHANNEL_IO>
        ether 78:4f:43:8d:4f:55
        inet6 fe80::1c20:1e66:6322:6ae9%en0 prefixlen 64 secured scopeid 0x5
        inet 192.168.1.167 netmask 0xffffff00 broadcast 192.168.1.255
        nd6 options=201<PERFORMNUD,DAD>
        media: autoselect
        status: active
```

In this example, I should use `192.168.1.167`.

### Special note: Docker on Hyper-V Linux VMs

For Docker running on a Linux VM on a Windows host under Hyper-V, the default gateway is usually
`172.18.0.1`.

## Step 3: Install and run Krustlet

First, install the latest release of Krustlet following [the install guide](../intro/install.md).

Once you have done that, run the following commands to run Krustlet's WASI provider:

```shell
# Since you are running locally, this step is important. Otherwise krustlet will pick up on your
# local config and not be able to update the node status properly
$ export KUBECONFIG=~/.krustlet/config/kubeconfig
$ krustlet-wasi --node-ip 172.17.0.1 --cert-file=~/.krustlet/config/krustlet.crt --private-key-file=~/.krustlet/config/krustlet.key --bootstrap-file=~/.krustlet/config/bootstrap.conf
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

Then, run `kubectl get nodes -o wide` and you should see output that looks similar to below:

```
NAME                 STATUS   ROLES    AGE     VERSION   INTERNAL-IP   EXTERNAL-IP   OS-IMAGE       KERNEL-VERSION     CONTAINER-RUNTIME
kind-control-plane   Ready    master   3m46s   v1.17.0   172.17.0.2    <none>        Ubuntu 19.10   5.3.0-42-generic   containerd://1.3.2
krustlet             Ready    agent    10s     v1.17.0   172.17.0.1    <none>        <unknown>      <unknown>          mvp
```
