# Running Krustlet on WSL2 with Docker Desktop

This how-to guide demonstrates how to boot a Krustlet node in Docker Desktop for Windows with WSL2
backend.

## Information
This tutorial will work on current Windows 10 Insider Slow ring and Docker Desktop for Windows
stable release.

Concerning Windows, this tutorial should work on the Production ring once it will be available.

Last but not least, this will work on Windows 10 Home edition.

## Prerequisites

You will require a WSL2 distro and Docker Desktop for Windows for this how-to. The WSL2 backend and
Kubernetes features will need to be also enabled. See the [Docker Desktop for Windows > Getting
started > Kubernetes](https://docs.docker.com/docker-for-windows/#kubernetes) howto for more
information.

This specific tutorial will be running Krustlet on your WSL2 distro and will explain how to access
it from Windows.

## Step 1: Determine the default gateway

The default gateway for most Docker containers is generally `172.17.0.1`. This IP is only reachable,
by default, from the WSL2 distro. However, the `eth0` host network is accessible from Windows, so we
can use this IP address to connect to the WSL2 distro (where Krustlet is running).

If this was changed, check `ifconfig eth0` from the host OS to determine the default gateway:

```console
$ ifconfig eth0
eth0: flags=4163<UP,BROADCAST,RUNNING,MULTICAST>  mtu 1500
        inet 172.26.47.208  netmask 255.255.240.0  broadcast 172.26.47.255
        inet6 fe80::215:5dff:fe98:ce48  prefixlen 64  scopeid 0x20<link>
        ether 00:15:5d:98:ce:48  txqueuelen 1000  (Ethernet)
        RX packets 16818  bytes 11576089 (11.0 MiB)
        RX errors 0  dropped 0  overruns 0  frame 0
        TX packets 1093  bytes 115724 (113.0 KiB)
        TX errors 0  dropped 0 overruns 0  carrier 0  collisions 0
```

In this example, I should use `172.26.47.208`.

> TIP: get the IP from `eth0`

```shell
$ export mainIP=$(ifconfig eth0 | grep "inet " | awk '{ print $2 }')
```

The hostname being "applied" from Windows, the default hostname will not resolve to this address,
therefore you need to pass the `--node-ip` and `--node-name` flag to Krustlet.

## Step 2: Get a bootstrap config

Krustlet requires a bootstrap token and config the first time it runs. Follow the guide
[here](bootstrapping.md) to generate a bootstrap config and then return to this document. This will
If you already have a kubeconfig available that you generated through another process, you can
proceed to the next step. However, the credentials Krustlet uses must be part of the `system:nodes`
group in order for things to function properly.

## Step 3: Install and run Krustlet

First, install the latest release of Krustlet following [the install guide](../intro/install.md).

Second, ensure the Kubernetes context is correctly set to `docker-desktop`:

```shell
$ kubectl config get-contexts
CURRENT   NAME                 CLUSTER          AUTHINFO         NAMESPACE
*         docker-desktop       docker-desktop   docker-desktop

# Optional if the context is not set correctly
$ kubectl config set-context docker-desktop
Context "docker-desktop" modified.
```

Once you have done that, run the following commands to run Krustlet's WASI provider:

```shell
# Since you are running locally, this step is important. Otherwise krustlet will pick up on your
# local config and not be able to update the node status properly
$ export KUBECONFIG=~/.krustlet/config/kubeconfig
$ krustlet-wasi --node-ip $mainIP --node-name krustlet --cert-file=~/.krustlet/config/krustlet.crt --private-key-file=~/.krustlet/config/krustlet.key --bootstrap-file=~/.krustlet/config/bootstrap.conf
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
$ kubectl get nodes -o wide
NAME             STATUS   ROLES    AGE     VERSION   INTERNAL-IP     EXTERNAL-IP   OS-IMAGE         KERNEL-VERSION                CONTAINER-RUNTIME
docker-desktop   Ready    master   3d23h   v1.15.5   192.168.65.3    <none>        Docker Desktop   4.19.104-microsoft-standard   docker://19.3.8
krustlet      Ready    agent    34s     v1.17.0   172.26.47.208   <none>        <unknown>        <unknown>                     mvp
```

## Optional: Delete the Krustlet node
Once you will no more need the Krustlet node, you can remove it from your cluster with the following
`kubectl delete node` command:

```shell
$ kubectl delete node krustlet
node "krustlet" deleted

$ kubectl get nodes -o wide
NAME             STATUS   ROLES    AGE   VERSION   INTERNAL-IP    EXTERNAL-IP   OS-IMAGE         KERNEL-VERSION                CONTAINER-RUNTIME
docker-desktop   Ready    master   4d    v1.15.5   192.168.65.3   <none>        Docker Desktop   4.19.104-microsoft-standard   docker://19.3.8
```

## Troubleshooting

### WASM workloads on Docker Desktop

Docker Desktop's Kubernetes always provides a schedulable node called
`docker-desktop`. This node uses Docker to run containers. If you want
to run WASM workloads on Krustlet, you must prevent these pods from being
scheduled to the `docker-desktop` node. You can do this using a nodeSelector
in pod specs. See [Running WASM workloads](../howto/wasm.md) for details.
