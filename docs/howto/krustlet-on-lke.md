$ Running Krustlet on Linode Kubernetes Engine (LKE)

These steps are for running a Krustlet node in a LKE cluster.

## Prequisites

You will require a LKE cluster. See the [how-to guide for running Kubernetes on
LKE](kubernetes-on-lke.md) for more information.

This specific tutorial will be running Krustlet on a Linode (VM); however
you may follow these steps from any device that can start a web server on an IP
accessible from the Kubernetes control plane.

In the [how-to guide for running Kubernetes on LKE](kubernetes-on-lke.md),
several environment variables were used. Let's reuse those values:

```console
$ CLUSTER_ID=[[CLUSTER-ID]]
$ REGION="us-west"
$ VERSION="1.18"
$ TYPE="g6-standard-1"
```

Let's confirm that the cluster exists:

```console
$ linode-cli lke cluster-view ${CLUSTER_ID}
```

## Step 1: Create Linode (VM)

In order to create a Linode, we will need to determine several values. These
values may be determined with the following commands:

```console
$ linode-cli images list
$ linode-cli regions list
$ linode-cli linodes types
```

It is recommended that you use SSH keys to authenticate against the Linode. The
[instructions](https://www.linode.com/docs/guides/use-public-key-authentication-with-ssh/) will guide you.

Please set the value of `PUBLIC_KEY` to point to the file containing the public
key you will use to authenticate the Linode:

You will also need to identify the Linode user. You can do this with the following
command. Set the value of `USER` to the user you wish to use:

```console
$ linode-cli show-users
```

You can create a Linode (VM) with the following command to create a Linode in the
`us-west` region using a `g6-standard-1` type with a randomly-generated root
password and your SSH public key:

```console
$ LABEL=[[YOUR-LABEL]]
$ TYPE="g6-standard-1"
$ REGION="us-west"
$ IMAGE="linode/debian10"
$ PASSWORD="$(</dev/urandom tr -dc a-z0-9 | head -c "${1:-32}";echo;)"

$ linode-cli linodes create \
  --type=${TYPE} \
  --region=${REGION} \
  --image=${IMAGE} \
  --root_pass=${PASSWORD} \
  --authorized_keys="$(cat ${PUBLIC_KEY})" \
  --booted=true \
  --label=${LABEL} \
  --authorized_users=${USER}
```

You may then grab the Linode's `ID` and public IP `IP` using:

```console
$ LINODE_ID=$(linode-cli linodes list \
  --label=${LABEL} \
  --format=id \
  --no-headers \
  --text) && echo ${LINODE_ID}

$ IP=$(linode-cli linodes list \
  --label=${LABEL} \
  --format=ipv4 \
  --no-headers \
  --text) && echo ${IP}
```

## Step 2: Get a bootstrap config for your Krustlet node

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

## Step 3: Copy bootstrap config to VM

The first thing we'll need to do is copy up the assets we generated in steps 1
and 2. Copy them to the Linode by typing:

```console
$ scp \
  -i ${PRIVATE_KEY} \
  ${HOME}/.krustlet/config/bootstrap.conf \
  root@${IP}:.
```

> **NOTE** You'll need to set `PRIVATE_KEY` to the path to the private key
corresponding to the SSH key pair that you used to create the Linode.

We can then SSH into the Linode by typing:

```console
$ ssh \
  -i ${PRIVATE_KEY} \
  root@${IP}
```

## Step 4: Install and configure Krustlet

Install the latest release of krustlet following [the install
guide](../intro/install.md).

There are two flavors of Krustlet (`krustlet-wasi` and `krustlet-wascc`), let's
use the first:

```console
$ KUBECONFIG=${PWD}/kubeconfig krustlet-wasi \
--hostname="krustlet" \
--node-ip=${IP} \
--node-name="krustlet" \
--bootstrap-file=./bootstrap.conf \
--cert-file=${PWD}/krustlet.crt \
--private-key-file=${PWD}/krustlet.key
```

**NOTE** To increase the level of debugging, you may prefix the command with
`RUST_LOG=info` or `RUST_LOG=debug`.

**NOTE** The value of `${IP}` was determined in step #1.

### Step 4a: Approving the serving CSR

Once you have started Krustlet, there is one more manual step (though this could
be automated depending on your setup) to perform. The client certs Krustlet
needs are generally approved automatically by the API. However, the serving
certs require manual approval. To do this, you'll need the hostname you
specified for the `--hostname` flag or the output of `hostname` if you didn't
specify anything. From another terminal that's configured to access the cluster,
run:

```console
$ kubectl certificate approve krustlet-tls
```

> **NOTE** You will only need to do this approval step the first time Krustlet
starts. It will generate and save all of the needed credentials to your machine

You should be able to enumerate the cluster's nodes including the Krustlet by
typing:

```console
$ kubectl get nodes
NAME                          STATUS   ROLES    AGE   VERSION
krustlet                      Ready    <none>   36s   0.5.0
lke12345-12345-1234567890ab   Ready    <none>   31m   v1.18.15
```

## Step 5: Test that things work

We may test that the Krustlet is working by running one of the demos:

```console
$ kubectl apply --filename=https://raw.githubusercontent.com/deislabs/krustlet/master/demos/wasi/hello-world-rust/k8s.yaml
$ kubectl get pods
NAME                    READY   STATUS       RESTARTS   AGE
hello-world-wasi-rust   0/1     ExitCode:0   0          2s
```

There's an issue with the `kubectl logs` command. If you try the following, you
will get an error:

```console
$ kubectl logs pods/hello-world-wasi-rust
Error from server: Get https://${IP}:3000/containerLogs/default/hello-world-wasi-rust/hello-world-wasi-rust
```

However, if you `curl` that endpoint, you should be able to retrieve the logs:

```console
$ curl --insecure https://${IP}:3000/containerLogs/default/hello-world-wasi-rust/hello-world-wasi-rust
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
Environment=KRUSTLET_NODE_IP=[[REPLACE-IP]]
Environment=KRUSTLET_NODE_NAME=krustlet
Environment=KRUSTLET_CERT_FILE=/etc/krustlet/config/krustlet.crt
Environment=KRUSTLET_PRIVATE_KEY_FILE=/etc/krustlet/config/krustlet.key
Environment=KRUSTLET_DATA_DIR=/etc/krustlet
Environment=RUST_LOG=wascc_provider=info,wasi_provider=info,main=info
Environment=KRUSTLET_BOOTSTRAP_FILE=/etc/krustlet/config/bootstrap.conf
ExecStart=/usr/local/bin/krustlet-wasi
User=root
Group=root

[Install]
WantedBy=multi-user.target
```

> **NOTE** Replace `[[REPLACE-IP]]` with the value of `${IP}`

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

## Delete the Linode

When you are finished with the Linode, you can delete it by typing:

```console
$ linode-cli linodes delete ${LINODE_ID}
```

When you are finished with the LKE cluster, you can delete it by typing:

```console
$ linode-cli lke cluster-delete ${CLUSTER_ID}
```