# Running Krustlet on Google Kubernetes Engine (GKE)

These steps are for running a Krustlet node in a GKE cluster.

## Prerequisites

You will require a GKE cluster. See the [how-to guide for running Kubernetes on GKE](kubernetes-on-gke.md)
for more information.

This specific tutorial will be running Krustlet on a Compute Engine VM; however you may follow
these steps from any device that can start a web server on an IP accessible from the Kubernetes
control plane.

In the [how-to guide for running Kubernetes on GKE](kubernetes-on-gke.md), several environment
variables were used to define a Google Cloud Platform project, region and Kubernetes Engine
cluster. Let's reuse those values:

```shell
$ PROJECT=[YOUR-PROJECT] # Perhaps $(whoami)-$(date +%y%m%d)-krustlet
$ REGION="us-west1" # Use a region close to you `gcloud compute regions list --project=${PROJECT}`
$ CLUSTER="cluster"
```

Let's confirm that the cluster exists. We can do this using either `gcloud` or `kubectl`:

```shell
$ gcloud container clusters describe ${CLUSTER} --project=${PROJECT} --region=${REGION}
$ gcloud container clusters describe ${CLUSTER} --project=${PROJECT} --region=${REGION} --format="value(status)"
RUNNING
```

Or:

```shell
$ kubectl get nodes
NAME                                     STATUS   ROLES    AGE     VERSION
gke-cluster-default-pool-1a3a5b85-scds   Ready    <none>   1m      v1.17.4-gke.10
gke-cluster-default-pool-3885c0e3-6zw2   Ready    <none>   1m      v1.17.4-gke.10
gke-cluster-default-pool-6d70a85d-19r8   Ready    <none>   1m      v1.17.4-gke.10
```

> **NOTE** If you chose to create a single-zone cluster, replace `--region=${REGION}` with
`--zone=${ZONE}` in the above `gcloud` commands.


## Step 1: Create Compute Engine VM

We can create a new VM with the following command:

```shell
$ INSTANCE="krustlet" # Name of this VM must matches the certificate's CN
$ # The cluster is distributed across the zones in the region
$ # For the VM, we'll pick one of the zones
$ ZONE="${REGION}-a" # Pick one of the zones in this region
$ gcloud beta compute instances create ${INSTANCE} \
--project=${PROJECT} \
--zone=${ZONE} \
--machine-type "n1-standard-1" \
--image-family="debian-10" \
--image-project="debian-cloud"
NAME      ZONE        MACHINE_TYPE   PREEMPTIBLE  INTERNAL_IP  EXTERNAL_IP     STATUS
krustlet  us-west1-a  n1-standard-1               xx.xx.xx.xx  yy.yy.yy.yy     RUNNING
```

It should take less than 30-seconds to provision the VM.

Let's determine the instance's internal (!) IP to use when creating the Kubernete certificate and
subsequently running Krustlet. In step #4, you'll need to copy this value into the command that is
used to run Krustlet on the VM:

```shell
$ IP=$(gcloud compute instances describe ${INSTANCE} \
--project=${PROJECT} \
--zone=${ZONE} \
--format="value(networkInterfaces[0].networkIP)") && echo ${IP}
```

## Step 2: Get a bootstrap config for your Krustlet node

Krustlet requires a bootstrap token and config the first time it runs. Follow the guide
[here](bootstrapping.md), setting the `CONFIG_DIR` variable to `./`, to generate a bootstrap config
and then return to this document. If you already have a kubeconfig available that you generated
through another process, you can proceed to the next step. However, the credentials Krustlet uses
must be part of the `system:nodes` group in order for things to function properly.

NOTE: You may be wondering why you can't run this on the VM you just provisioned. We need access to
the Kubernetes API in order to create the bootstrap token, so the script used to generate the
bootstrap config needs to be run on a machine with the proper Kubernetes credentials

## Step 3: Copy bootstrap config to VM

The first thing we'll need to do is copy up the assets we generated in steps 1 and 2. Copy them to
the VM by typing:

```shell
$ gcloud compute scp bootstrap.conf ${INSTANCE}: --project=${PROJECT} --zone=${ZONE}
```

We can then SSH into the instance by typing:

```shell
$ gcloud compute ssh ${INSTANCE} --project=${PROJECT} --zone=${ZONE}
```

## Step 4: Install and configure Krustlet

Install the latest release of krustlet following [the install guide](../intro/install.md).

There are two flavors of Krustlet (`krustlet-wasi` and `krustlet-wascc`), let's use the first:

```shell
$ KUBECONFIG=${PWD}/kubeconfig krustlet-wasi \
--hostname="krustlet" \
--node-ip=${IP} \
--node-name="krustlet" \
--cert-file=./krustlet.crt \
--private-key-file=./krustlet.key \
--bootstrap-file=./bootstrap.conf
```

> **NOTE** To increase the level of debugging, you may prefix the command with `RUST_LOG=info` or
`RUST_LOG=debug`.

> **NOTE** The value of `${IP}` was determined in step #1.

### Step 4a: Approving the serving CSR

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

You should be able to enumerate the cluster's nodes including the Krustlet by typing:

```shell
$ kubectl get nodes
NAME                                     STATUS   ROLES    AGE   VERSION
gke-cluster-default-pool-1a3a5b85-scds   Ready    <none>   59m   v1.17.4-gke.10
gke-cluster-default-pool-3885c0e3-6zw2   Ready    <none>   36m   v1.17.4-gke.10
gke-cluster-default-pool-6d70a85d-19r8   Ready    <none>   59m   v1.17.4-gke.10
krustlet                                 Ready    agent    8s    v1.17.0
```

## Step 5: Test that things work

We may test that the Krustlet is working by running one of the demos:

```shell
$ kubectl apply --filename=https://raw.githubusercontent.com/deislabs/krustlet/master/demos/wasi/hello-world-rust/k8s.yaml
$ # wait a few seconds for the pod to run
$ kubectl logs pods/hello-world-wasi-rust
hello from stdout!
hello from stderr!
CONFIG_MAP_VAL=cool stuff
FOO=bar
POD_NAME=hello-world-wasi-rust
Args are: []
```

> **NOTE** you may receive an `ErrImagePull` and `Failed to pull image` and
`failed to generate container`. This results if the taints do not apply correctly. You should be
able to resolve this issue, using the following YAML.

```YAML
apiVersion: v1
kind: ConfigMap
metadata:
  name: hello-world-wasi-rust
data:
  myval: "cool stuff"
---
apiVersion: v1
kind: Pod
metadata:
  name: hello-world-wasi-rust
spec:
  containers:
    - name: hello-world-wasi-rust
      image: webassembly.azurecr.io/hello-world-wasi-rust:v0.1.0
      env:
        - name: FOO
          value: bar
        - name: POD_NAME
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: CONFIG_MAP_VAL
          valueFrom:
            configMapKeyRef:
              key: myval
              name: hello-world-wasi-rust
  nodeSelector:
    kubernetes.io/arch: "wasm32-wasi"
  tolerations:
    - key: "krustlet/arch"
      operator: "Equal"
      value: "wasm32-wasi"
      effect: "NoExecute"
    - key: "node.kubernetes.io/network-unavailable"
      operator: "Exists"
      effect: "NoSchedule"
```

## Step 6: Run Krustlet as a service

Create `krustlet.service` in `/etc/systemd/system/krustlet.service` on the VM.

```
[Unit]
Description=Krustlet, a kubelet implementation for running WASM

[Service]
Restart=on-failure
RestartSec=5s
Environment=KUBECONFIG=/etc/krustlet/config/kubeconfig
Environment=NODE_NAME=krustlet
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

Ensure that the `krustlet.service` has the correct ownership and permissions with:

```shell
$ sudo chown root:root /etc/systemd/system/krustlet.service
$ sudo chmod 644 /etc/systemd/system/krustlet.service
```

Then:

```shell
$ sudo mkdir -p /etc/krustlet/config && sudo chown -R root:root /etc/krustlet
$ sudo mv {krustlet.*,kubeconfig} /etc/krustlet && chmod 600 /etc/krustlet/*
```

Once you have done that, run the following commands to make sure the unit is configured to start on
boot:

```shell
$ sudo systemctl enable krustlet && sudo systemctl start krustlet
```

## Delete the VM

When you are finished with the VM, you can delete it by typing:

```shell
$ gcloud compute instances delete ${INSTANCE} --project=${PROJECT} --zone=${ZONE} --quiet
```

When you are finished with the cluster, you can delete it by typing:

```shell
$ # If you created a regional cluster
$ gcoud container clusters delete ${CLUSTER} --project=${PROJECT} --region=${REGION}
$ # If you created a zonal cluster
$ gcoud container clusters delete ${CLUSTER} --project=${PROJECT} --zone=${ZONE}
```
