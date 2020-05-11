# Running Krustlet on Google Kubernetes Engine (GKE)

These steps are for running a Krustlet node in a GKE cluster.

## Prerequisites

You will require a GKE cluster. See the [how-to guide for running Kubernetes on GKE](kubernetes-on-gke.md) for more information.

This specific tutorial will be running Krustlet on a Compute Engine VM; however you may follow these steps from any device that can start a web server on an IP accessible from the Kubernetes control plane.

In the [how-to guide for running Kubernetes on GKE](kubernetes-on-gke.md), several environment variables were used to define a Google Cloud Platform project, region and Kubernetes Engine cluster. Let's reuse those values:

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

> **NOTE** If you chose to create a single-zone cluster, replace `--region=${REGION}` with `--zone=${ZONE}` in the above `gcloud` commands.

## Step 1: Create a service account user for the node

We will create a service account for Krustlet to use to register nodes and access specific secrets.

This can be done by using the Kubernetes manifest in the [assets](./assets) directory:

```shell
$ kubectl apply --namespace=kube-system --filename=./docs/howto/assets/krustlet-service-account.yaml
```

Or by using the manifest directly from GitHub:

```shell
$ kubectl apply \
--namespace=kube-system \
--filename=https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/krustlet-service-account.yaml
```

Now that things are all set up, we need to generate the kubeconfig. You can do this by running
(assuming you are in the root of the krustlet repo):

```shell
$ ./docs/howto/assets/generate-kubeconfig.sh
```

Or if you are feeling more trusting, you can run it straight from the repo:

```shell
bash <(curl https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/generate-kubeconfig.sh)
```

Either way, it will output a file called `kubeconfig-sa` in your current directory. Save this for
later.

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
instance  us-west1-a  n1-standard-1               xx.xx.xx.xx  yy.yy.yy.yy     RUNNING
```

Let's determine the instance's internal (!) IP to use when creating the Kubernete certificate and subsequently running Krustlet. In step #4, you'll need to copy this value into the command that is used to run Krustlet on the VM:

```shell
$ IP=$(gcloud compute instances describe ${INSTANCE} \
--project=${PROJECT} \
--zone=${ZONE} \
--format="value(networkInterfaces[0].networkIP)") && echo ${IP}
```

It should take less than 30-seconds to provision the VM.

## Step 2: Create Certificate

Krustlet requires a certificate for securing communication with the Kubernetes API. Because
Kubernetes has its own certificates, we'll need to get a signed certificate from the Kubernetes API
that we can use.

In order for the Kubernetes cluster to resolve the name of the VM running the Krustlet, we'll include aliases for the VM's name in the certificate.

First things first, let's create a certificate signing request (CSR):

```shell
$ ALIASES="DNS:${INSTANCE},DNS:${INSTANCE}.${ZONE},IP:${IP}" && echo ${ALIASES}
$ openssl req -new -sha256 -newkey rsa:2048 -keyout ./krustlet.key -out ./krustlet.csr -nodes -config <(
cat <<-EOF
[req]
default_bits = 2048
prompt = no
default_md = sha256
req_extensions = req_ext
distinguished_name = dn
[dn]
O=.
CN=${INSTANCE}
[req_ext]
subjectAltName = ${ALIASES}
EOF
)
Generating a RSA private key
....................+++++
............................................+++++
writing new private key to './krustlet.key'
```

This will create a CSR and a new key for the certificate. Now that it is created, we'll need to send the request to Kubernetes:

```shell
$ cat <<EOF | kubectl apply --filename=-
apiVersion: certificates.k8s.io/v1beta1
kind: CertificateSigningRequest
metadata:
  name: krustlet
spec:
  request: $(cat krustlet.csr | base64 | tr -d '\n')
  usages:
  - digital signature
  - key encipherment
  - server auth
EOF
certificatesigningrequest.certificates.k8s.io/krustlet created
```

You should then approve the request:

```shell
$ kubectl certificate approve krustlet
certificatesigningrequest.certificates.k8s.io/krustlet approved
```

After approval, you can download the cert like so:

```shell
$ kubectl get csr krustlet --output=jsonpath='{.status.certificate}' \
    | base64 --decode > krustlet.crt
```

Lastly, combine the key and the cert into a PFX bundle, choosing your own password instead of
"password":

```shell
$ openssl pkcs12 -export -out krustlet.pfx -inkey krustlet.key -in krustlet.crt -password "pass:password"
```

## Step 3: Copy assets to VM

The first thing we'll need to do is copy up the assets we generated in steps 1 and 2. Copy them to the VM by typing:

```shell
$ gcloud compute scp krustlet.pfx ${INSTANCE}: --project=${PROJECT} --zone=${ZONE}
$ gcloud compute scp kubeconfig-sa ${INSTANCE}: --project=${PROJECT} --zone=${ZONE}
```

We can then SSH into the instance by typing:

```shell
$ gcloud compute ssh ${INSTANCE} --project=${PROJECT} --zone=${ZONE}
```

## Step 4: Install and configure Krustlet

Install the latest release of krustlet following [the install guide](../intro/install.md).

There are two flavors of Krustlet (`krustlet-wasi` and `krustlet-wascc`), let's use the first:

```shell
$ KUBECONFIG=${PWD}/kubeconfig-sa ./krustlet-wasi \
--hostname="krustlet" \
--node-ip=${IP} \
--node-name="krustlet" \
--pfx-password="password" \
--pfx-path=./krustlet.pfx
```

> **NOTE** To increase the level of debugging, you may prefix the command with `RUST_LOG=info` or `RUST_LOG=debug`.

> **NOTE** The value of `${IP}` was determined in step #1.



From another terminal that's configured to access the cluster, you should be able to enumerate the cluster's nodes including the Krustlet by typing:

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
$ wait a few seconds for the pod to run
$ kubectl logs pods/hello-world-wasi-rust
hello from stdout!
hello from stderr!
CONFIG_MAP_VAL=cool stuff
FOO=bar
POD_NAME=hello-world-wasi-rust
Args are: []
```

> **NOTE** you may receive an `ErrImagePull` and `Failed to pull image` and `failed to generate container`. This results if the taints do not apply correctly. You should be able to resolve this issue, using the following YAML.

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

## Delete the VM

When you are finished with the VM, you can delete it by typing:

```shell
$ gcloud compute instances delete ${INSTANCE} --project=${PROJECT} --zone=${ZONE} --quiet
```

When you are finished with the cluster, you can delete it by typing:

```shell
$ If you created a regional cluster
$ gcoud container clusters delete ${CLUSTER} --project=${PROJECT} --region=${REGION}
$ If you created a zonal cluster
$ gcoud container clusters delete ${CLUSTER} --project=${PROJECT} --zone=${ZONE}
```
