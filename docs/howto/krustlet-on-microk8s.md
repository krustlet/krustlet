# Running Krustlet on [MicroK8s](https://microk8s.io)

These are steps for running Krustlet node(s) and [MicroK8s](https://microk8s.io) on the same machine.

## Prerequisites

You will require a running MicroK8s cluster for this guide. The steps below assume you will run
MicroK8s and the Krustlet, on a single machine. `kubectl` is required but is installed with MicroK8s
as `microk8s.kubectl`. The following instructions use `microk8s.kubectl` for simplicity.
You may use a standlone `kubectl` if you prefer.

## Step 1: Create a service account user for the node

We need to edit the Kubernetes configuration file (known as the kubeconfig) and create a service account
for Krustlet to use to register nodes and access specific secrets.

The service account can be created by using the Kubernetes manifest in the [assets](./assets) directory:

```shell
$ microk8s.kubectl apply --namespace=kube-system --filename=./docs/howto/assets/krustlet-service-account.yaml
```

You can also do this by using the manifest straight from GitHub:

```shell
$ microk8s.kubectl apply --namespace=kube-system --filename=https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/krustlet-service-account.yaml
```
The following commands edit Kubernetes configuration file, adding a context that will be used by Krustlet:

```shell
SERVICE_ACCOUNT_NAME="krustlet"
CONTEXT="krustlet"
CLUSTER="microk8s-cluster"
NAMESPACE="kube-system"
USER="${CONTEXT}-token-user"

SECRET_NAME=$(microk8s.kubectl get serviceaccount/${SERVICE_ACCOUNT_NAME} \
  --namespace=${NAMESPACE} \
  --output=jsonpath='{.secrets[0].name}')
TOKEN_DATA=$(microk8s.kubectl get secret/${SECRET_NAME} \
  --namespace=${NAMESPACE} \
  --output=jsonpath='{.data.token}')

TOKEN=$(echo ${TOKEN_DATA} | base64 -d)

# Create user
microk8s.kubectl config set-credentials ${USER} \
--token ${TOKEN}
# Create context
microk8s.kubectl config set-context ${CONTEXT}
# Set context to use cluster, namespace and user
microk8s.kubectl config set-context ${CONTEXT} \
--cluster=${CLUSTER} \
--user=${USER} \
--namespace=${NAMESPACE}
```

> **NOTE** We'll switch to this context when we run Krustlet; for now we'll continue using the
current (probably 'default') context

## Step 2: Create Certificate

Krustlet requires a certificate for securing communication with the Kubernetes API. Because
Kubernetes has its own certificates, we'll need to get a signed certificate from the Kubernetes
API that we can use. First things first, let's create a certificate signing request (CSR):

```shell
$ openssl req -new -sha256 -newkey rsa:2048 -keyout krustlet.key -out krustlet.csr -nodes -subj "/C=US/ST=./L=./O=./OU=./CN=krustlet"
```

This will create a CSR and a new key for the certificate, using `krustlet` as the hostname of the
server.

Now that it is created, we'll need to send the request to Kubernetes:

```shell
$ cat <<EOF | microk8s.kubectl apply --filename=-
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

Once that runs, an admin (that is probably you! at least it should be if you are trying to add a
node to the cluster) needs to approve the request:

```shell
$ microk8s.kubectl certificate approve krustlet
certificatesigningrequest.certificates.k8s.io/krustlet approved
```

After approval, you can download the cert like so:

```shell
$ microk8s.kubectl get csr krustlet --output=jsonpath='{.status.certificate}' \
    | base64 --decode > krustlet.crt
```

## Step 3: Install and configure Krustlet

Install the latest release of Krustlet following [the install guide](../intro/install.md).

We want the Krustlet to run as the service account that we created in step #1. This is configured
by the context (`krustlet`) that we created in that step. Unfortunately, it's not possible to
reference a specific context, so we must change the context before running Krustlet:

```shell
$ microk8s.kubectl config use-context ${CONTEXT}
Switched to context "krustlet".
```

There are 2 binaries (`krustlet-wasi` and `krustlet-wascc`), let's start the first:

```shell
$ KUBECONFIG=/var/snap/microk8s/current/credentials/client.config \
./krustlet-wasi \
--node-ip=127.0.0.1 \
--node-name=krustlet \
--tls-cert-file=./krustlet.crt \
--tls-private-key-file=./krustlet.key
```

In another terminal:

We'll ensure the Kubernetes context is reverted to the default (`microk8s`) before proceeding:

```shell
$ microk8s.kubectl use context microk8s
Switched to context "microk8s".
```

```shell
$ microk8s.kubectl get nodes --output=wide
NAME                                STATUS   ROLES   AGE     VERSION   INTERNAL-IP   EXTERNAL-IP   OS-IMAGE             KERNEL-VERSION      CONTAINER-RUNTIME
krustlet                            Ready    agent   11s     v1.17.0   127.0.0.1     <none>        <unknown>            <unknown>           mvp
microk8s                            Ready    <none>  13m     v1.18.2   10.138.0.4    <none>        Ubuntu 20.04 LTS     5.4.0-1009-gcp      containerd://1.2.5
```

## Step 4: Test that things work

We'll ensure the Kubernetes context is reverted to the default (`microk8s`) before proceeding:

```shell
$ microk8s.kubectl use context microk8s
Switched to context "microk8s".
```

Now you can see things work! Feel free to give any of the demos a try like so:

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
