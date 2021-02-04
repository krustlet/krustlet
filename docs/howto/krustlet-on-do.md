# Running Krustlet on Managed Kubernetes on DigitalOcean

These steps are for running a Krustlet node on a DigitalOcean Droplet in a
Managed Kubernetes DigitalOcean cluster.

## Prerequisites

You will require a Managed Kubernetes on DigitalOcean cluster. See the [how-to
guide for running Managed Kubernetes on DigitalOcean](kubernetes-on-do.md) for
more information.

This tutorial runs Krustlet on a DigitalOcean Droplet (VM); however you may
follow these steps from any device that can start a web server on an IP
accessible from the Kubernetes control plane.

In the [how-to guide for running Managed Kubernetes on DigitalOcean](kubernetes-on-do.md),
several environment variables were used to define the cluster. Let's reuse
those values:

```console
$ CLUSTER=[[YOUR-CLUSTER-NAME]]
$ VERSION="1.19.3-do.3"
$ SIZE="s-1vcpu-2gb"
$ REGION="sfo3"
```

Let's also confirm that the cluster exists:

```console
$ doctl kubernetes cluster list
```

## Step 1: Create DigitalOcean Droplet (VM)

As with the cluster, there are several values (size, region) that you will need
to determine before you create the Droplet. `doctl compute` includes commands
to help you determine slugs for these values:

```console
$ doctl compute size list
$ doctl compute region list
$ doctl compute image list --public
```

If you'd prefer, you may use the values below. However, it is strongly
recommended that you use SSH keys to authenticate with Droplets. DigitalOcean
provides [instructions](https://www.digitalocean.com/docs/droplets/how-to/add-ssh-keys/).

You may then list your SSH keys:

```console
$ doctl compute ssh-key list
```

> **NOTE** In this case, you reference the key using an `ID` value

We can create a new DigitalOcean Droplet using the following command:

```console
$ INSTANCE=[[YOUR-INSTANCE-NAME]]
$ SIZE="s-1vcpu-2gb"   # Need not be the same size as the cluster node(s)
$ REGION="sfo3"        # Need not be the same region as the cluster
IMAGE="debian-10-x64"
SSH_KEY=[[YOUR-SSH-KEY]]

doctl compute droplet create ${INSTANCE} \
--region ${REGION} \
--size ${SIZE} \
--ssh-keys ${SSH_KEY} \
--tag-names krustlet,wasm \
--image ${IMAGE}
```

> **NOTE** The service will response with an `ID` value for the Droplet. As long
as the Droplet name (`INSTANCE`) is unique, you may refer to the Droplet by its
name value too.

You will need the Droplet's IPv4 public address so make a note of it (`IP`):

```console
$ doctl compute droplet get ${INSTANCE} \
  --format PublicIPv4 \
  --no-header
```

## Step 2: Get a bootstrap config for your Kustlet node

Krustlet requires a bootstrap token and config the first time it runs. Follow
the guide [here](bootstrapping.md), setting the `CONFIG_DIR` variable to `./`,
to generate a bootstrap config and then return to this document. If you already
have a kubeconfig available that you generated through another process, you can
proceed to the next step. However, the credentials Krustlet uses must be part of
the `system:nodes` group in order for things to function properly.

NOTE: You may be wondering why you can't run this on the VM you just
provisioned. We need access to the Kubernetes API in order to create the
bootstrap token, so the script used to generate the bootstrap config needs to be
run on a machine with the proper Kubernetes credentials.

## Step 3: Copy bootstrap config to Droplet

The first thing we'll need to do is copy up the assets we generated in steps 1
and 2. Copy them to the VM by typing:

```console
scp -i ${PRIVATE_KEY} \
  ${HOME}/.krustlet/config/bootstrap.conf \
  root@${IP}:.
```

> **NOTE** `IP` is the Droplet's IPv4 address from step #1 and `PRIVATE_KEY` is
the location of the file containing the private (!) key that corresponds to the
public key that you used when you created the Droplet.

We can then SSH into the Droplet by typing:

```console
$ doctl compute ssh ${INSTANCE} \
  --ssh-key-path ${PRIVATE_KEY}
```

If you'd prefer, you may use `ssh` directly:

```console
$ ssh -i ${PRIVATE_KEY} root@${IP}
```

## Step 4: Install and configure Kruslet

Install the latest release of krustlet following [the install
guide](../intro/install.md).

Let's use the built-in `krustlet-wasi` provider:

```console
$ KUBECONFIG=${PWD}/kubeconfig ${PWD}/krustlet-wasi \
  --node-ip=${IP} \
  --node-name="krustlet" \
  --bootstrap-file=${PWD}/bootstrap.conf \
  --cert-file=${PWD}/krustlet.crt \
  --private-key-file=${PWD}/krustlet.key
```

> **NOTE** You'll need the `IP` of the Droplet from step 1.
> **NOTE** To increase the level of debugging, you may prefix the command with
`RUST_LOG=info` or `RUST_LOG=debug`.

If you restart the Krustlet after successfully (!) bootstrapping, you may run:

```console
$ KUBECONFIG=${PWD}/kubeconfig ${PWD}/krustlet-wasi \
  --node-ip=${IP} \
  --node-name="krustlet" \
  --cert-file=${PWD}/krustlet.crt \
  --private-key-file=${PWD}/krustlet.key
```

If bootstrapping fails, you should delete the CSR and try to bootstrap again:

```console
$ kubectl delete csr ${INSTANCE}-tls
```

## Step 4a: Approving the serving CSR

Once you have started Krustlet, there is one more manual step (though this could
be automated depending on your setup) to perform. The client certs Krustlet
needs are generally approved automatically by the API. However, the serving
certs require manual approval. To do this, you'll need the hostname you
specified for the `--hostname` flag or the output of `hostname` if you didn't
specify anything. From another terminal that's configured to access the cluster,
run:

```console
$ kubectl certificate approve ${INSTANCE}-tls
```

> **NOTE** You will only need to do this approval step the first time Krustlet
starts. It will generate and save all of the needed credentials to your machine.

You should be able to enumerate the cluster's nodes including the Krustlet by
typing:

```console
$ kubectl get nodes
NAME                           STATUS   ROLES    AGE   VERSION
krustlet                       Ready    <none>   60s   0.5.0
${CLUSTER}-default-pool-39yh5  Ready    <none>   10m   v1.19.3
```

## Step 5: Test that things work

We may test that the Krustlet is working by running one of the demos:

```console
$ kubectl apply --filename=https://raw.githubusercontent.com/deislabs/krustlet/master/demos/wasi/hello-world-rust/k8s.yaml
$ kubectl get pods
NAME                    READY   STATUS       RESTARTS   AGE
hello-world-wasi-rust   0/1     ExitCode:0   0          12s

$ kubectl logs pods/hello-world-wasi-rust
hello from stdout!
hello from stderr!
FOO=bar
CONFIG_MAP_VAL=cool stuff
POD_NAME=hello-world-wasi-rust
Args are: []

Bacon ipsum dolor amet chuck turducken porchetta, tri-tip spare ribs t-bone ham hock. Meatloaf
pork belly leberkas, ham beef pig corned beef boudin ground round meatball alcatra jerky.
Pancetta brisket pastrami, flank pork chop ball tip short loin burgdoggen. Tri-tip kevin
shoulder cow andouille. Prosciutto chislic cupim, short ribs venison jerky beef ribs ham hock
short loin fatback. Bresaola meatloaf capicola pancetta, prosciutto chicken landjaeger andouille
swine kielbasa drumstick cupim tenderloin chuck shank. Flank jowl leberkas turducken ham tongue
beef ribs shankle meatloaf drumstick pork t-bone frankfurter tri-tip.
```

## Step 6: Run Krustlet as a service

Create `krustlet.service` in `/etc/systemd/system/krustlet.service` on the VM.

```text
[Unit]
Description=Krustlet, a kubelet implementation for running WASM

[Service]
Restart=on-failure
RestartSec=5s
Environment=KUBECONFIG=/etc/krustlet/config/kubeconfig
Environment=KRUSTLET_NODE_IP=[[REPLACE-WITH-IP]]
Environment=KRUSTLET_NODE_NAME=krustlet
Environment=KRUSTLET_CERT_FILE=/etc/krustlet/config/krustlet.crt
Environment=KRUSTLET_PRIVATE_KEY_FILE=/etc/krustlet/config/krustlet.key
Environment=KRUSTLET_DATA_DIR=/etc/krustlet
Environment=RUST_LOG=wasi_provider=info,main=info
ExecStart=/usr/local/bin/krustlet-wasi
User=root
Group=root

[Install]
WantedBy=multi-user.target
```

Ensure that the `krustlet.service` has the correct ownership and permissions
with:

```console
$ sudo chown root:root /etc/systemd/system/krustlet.service
$ sudo chmod 644 /etc/systemd/system/krustlet.service
```

Then:

```console
$ sudo mkdir -p /etc/krustlet/config && sudo chown -R root:root /etc/krustlet
$ sudo mv {krustlet.*,kubeconfig} /etc/krustlet/config && chmod 600 /etc/krustlet/*
```

Once you have done that, run the following commands to make sure the unit is
configured to start on boot:

```console
$ sudo systemctl enable krustlet && sudo systemctl start krustlet
```

You may confirm the status of the service and review logs using:

```console
$ systemctl status krustlet.service
$ journalctl --unit=krustlet.service --follow

## Delete the VM

When you are finished with the VM, you can delete it by typing:

```console
$ doctl compute droplet delete ${INSTANCE}
```

When you are finished with the cluster, you can delete it by typing:

```console
$ doctl kubernetes cluster delete ${CLUSTER}
```

`doctl kubernetes cluster delete` will also attempt to delete the cluster's
configuration (cluster, context, user) from the default Kubernetes config file
(Linux: `${HOME}/.kube/config`). You will neeed to set a new default context.
