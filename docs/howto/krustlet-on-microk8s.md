# Running Krustlet on [MicroK8s](https://microk8s.io)

These are steps for running Krustlet node(s) and [MicroK8s](https://microk8s.io) on the same machine.

## Prerequisites

You will require a running MicroK8s cluster for this guide. The steps below assume you will run
MicroK8s and the Krustlet, on a single machine. `kubectl` is required but is installed with MicroK8s
as `microk8s.kubectl`. The following instructions use `microk8s.kubectl` for simplicity.
You may use a standlone `kubectl` if you prefer.

In order for the bootstrap authentication token to work, your kube-apiserver needs to have
the `--enable-bootstrap-token-auth` feature flag enabled.
See [bootstrap-tokens](https://kubernetes.io/docs/reference/access-authn-authz/bootstrap-tokens/)
for more information.

To verify you have the bootstrap authentication feature enabled, check the process args:

```console
$ ps -ef | grep kube-apiserver | grep "enable-bootstrap-token-auth"
```

If it doesn't show up and you installed using `snap`, you can find the startup args in
`/var/snap/microk8s/current/args/kube-apiserver` and add the flag.

Now you need to
[restart](https://microk8s.io/docs/configuring-services) the kube-apiserver with the command:

```console
$ systemctl restart snap.microk8s.daemon-apiserver
```

## Step 1: Get a bootstrap config

Krustlet requires a bootstrap token and config the first time it runs. Follow the guide
[here](bootstrapping.md) to generate a bootstrap config and then return to this document. This will
If you already have a kubeconfig available that you generated through another process, you can
proceed to the next step. However, the credentials Krustlet uses must be part of the `system:nodes`
group in order for things to function properly.

## Step 2: Install and configure Krustlet

Install the latest release of Krustlet following [the install guide](../intro/install.md).


There are 2 binaries (`krustlet-wasi` and `krustlet-wascc`), let's start the first:

```shell
$ KUBECONFIG=~/.krustlet/config \
./krustlet-wasi \
--node-ip=127.0.0.1 \
--node-name=krustlet \
--cert-file=~/.krustlet/config/krustlet.crt \
--private-key-file=~/.krustlet/config/krustlet.key \
--bootstrap-file=~/.krustlet/config/bootstrap.conf
```

### Step 2a: Approving the serving CSR

Once you have started Krustlet, there is one more manual step (though this could be automated
depending on your setup) to perform. The client certs Krustlet needs are generally approved
automatically by the API. However, the serving certs require manual approval. To do this, you'll
need the hostname you specified for the `--hostname` flag or the output of `hostname` if you didn't
specify anything. From another terminal that's configured to access the cluster, run:

```bash
$ microk8s.kubectl certificate approve <hostname>-tls
```

NOTE: You will only need to do this approval step the first time Krustlet starts. It will generate
and save all of the needed credentials to your machine

## Step 3: Test that things work

Now you can see things work! Feel free to give any of the demos a try in another terminal like so:

```shell
$ microk8s.kubectl apply --file=https://raw.githubusercontent.com/deislabs/krustlet/master/demos/wasi/hello-world-rust/k8s.yaml
$ microk8s.kubectl logs pod/hello-world-wasi-rust
hello from stdout!
hello from stderr!
CONFIG_MAP_VAL=cool stuff
FOO=bar
POD_NAME=hello-world-wasi-rust
Args are: []
```
