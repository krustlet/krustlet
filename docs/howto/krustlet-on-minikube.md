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

## Step 1: Create Certificate

Krustlet requires a certificate for securing communication with the Kubernetes API. Because
Kubernetes has its own certificates, we'll need to get a signed certificate from the Kubernetes API
that we can use. First things first, let's create a certificate signing request (CSR):

```shell
$ mkdir -p ~/.krustlet/config
$ cd $_
$ openssl req -new -sha256 -newkey rsa:2048 -keyout krustlet.key -out krustlet.csr -nodes -subj "/C=US/ST=./L=./O=./OU=./CN=krustlet"
Generating a RSA private key
.................+++++
....................................................+++++
writing new private key to 'krustlet.key'
```

This will create a CSR and a new key for the certificate, using `krustlet` as the hostname of the
server.

Now that it is created, we'll need to send the request to Kubernetes:

```shell
$ cat <<EOF | kubectl apply -f -
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
$ kubectl certificate approve krustlet
certificatesigningrequest.certificates.k8s.io/krustlet approved
```

After approval, you can download the cert like so:

```shell
$ kubectl get csr krustlet -o jsonpath='{.status.certificate}' | base64 --decode > krustlet.crt
```

## Step 2: Determine the default gateway

The default gateway when you [set up minikube with the VirtualBox driver](kubernetes-on-minikube.md)
is generally `10.0.2.2`. We can use this IP address from the guest Operating System (the minikube
host) to connect to the host Operating System (where Krustlet is running). If this was changed, use
`minikube ssh` and `ip addr show` from the guest OS to determine the default gateway.

## Step 3: Install and run Krustlet

First, install the latest release of Krustlet following [the install guide](../intro/install.md).

Once you have done that, run the following commands to run Krustlet's WASI provider:

```shell
$ krustlet-wasi --node-ip 10.0.2.2 --pfx-password password --tls-cert-file=./krustlet.crt --tls-private-key-file=./krustlet.key
```

In another terminal, run `kubectl get nodes -o wide` and you should see output that looks similar to
below:

```
NAME       STATUS   ROLES    AGE   VERSION   INTERNAL-IP      EXTERNAL-IP   OS-IMAGE               KERNEL-VERSION   CONTAINER-RUNTIME
minikube   Ready    master   18m   v1.18.0   192.168.99.165   <none>        Buildroot 2019.02.10   4.19.107         docker://19.3.8
krustlet   Ready    agent    9s    v1.17.0   10.0.2.2         <none>        <unknown>              <unknown>        mvp
```
