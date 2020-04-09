# Running Krustlet on WSL2 with Docker Desktop

This how-to guide demonstrates how to boot a Krustlet node in Docker Desktop for Windows with WSL2 backend.

## Information
This tutorial will work on current Windows 10 Insider Slow ring and Docker Desktop for Windows stable release.

Concerning Windows, this tutorial should work on the Production ring once it will be available.

Last but not least, this will work on Windows 10 Home edition.

## Prerequisites

You will require a WSL2 distro and Docker Desktop for Windows for this how-to. The WSL2 backend and Kubernetes features will need to be also enabled.
See the [Docker Desktop for Windows > Getting started > Kubernetes](https://docs.docker.com/docker-for-windows/#kubernetes) howto for more information.

This specific tutorial will be running Krustlet on your WSL2 distro and will explain how to access it from Windows.

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

Once that runs, you will need to approve the request:

```shell
$ kubectl certificate approve krustlet
certificatesigningrequest.certificates.k8s.io/krustlet approved
```

After approval, you can download the cert like so:

```shell
$ kubectl get csr krustlet -o jsonpath='{.status.certificate}' | base64 --decode > krustlet.crt
```

Lastly, combine the key and the cert into a PFX bundle, choosing your own password instead of
"password":

```shell
$ openssl pkcs12 -export -out certificate.pfx -inkey krustlet.key -in krustlet.crt -password "pass:password"
```

## Step 2: Determine the default gateway

The default gateway for most Docker containers is generally `172.17.0.1`.
This IP is only reachable, by default, from the WSL2 distro.
However, the `eth0` host network is accessible from Windows, so we can use this IP address to connect to the WSL2 distro (where Krustlet is running). 

If this was changed, check `ifconfig eth0` from
the host OS to determine the default gateway:

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

The hostname being "applied" from Windows, the default hostname will not resolve to this address, therefore you need to pass the `--node-ip` and `--node-name` flag to Krustlet.

### Add a route to 172.17.0.1

As stated above, the Docker default gateway, `172.17.0.1`, cannot be reached from Windows by default.

However, a route can create to reach it, using the WSL2 own default gateway.

We can use the following commands to create a temporary route:

```shell
PS> $env:WSLIP = Get-NetIPConfiguration -InterfaceAlias *WSL* | % { $_.IPv4Address.IPAddress }

PS> $env:WSLIP = Get-NetIPConfiguration -InterfaceAlias *WSL* | % { $_.IPv4Address.IPAddress }
 OK!
```

**DO NOT** make this route permanent as the WSL2 default gateway is DHCP based and will change upon every reboot.

## Step 3: Install and run Krustlet

First, install the latest release of Krustlet following [the install guide](../intro/install.md).

Once you have done that, run the following commands to run Krustlet's WASI provider:

```shell
$ krustlet-wasi --node-ip 172.17.0.1 --node-name krustlet --pfx-password password
```

In another terminal, run `kubectl get nodes -o wide` and you should see output that looks similar to
below:

```
$ kubectl get nodes -o wide
NAME             STATUS   ROLES    AGE     VERSION   INTERNAL-IP     EXTERNAL-IP   OS-IMAGE         KERNEL-VERSION                CONTAINER-RUNTIME
docker-desktop   Ready    master   3d23h   v1.15.5   192.168.65.3    <none>        Docker Desktop   4.19.104-microsoft-standard   docker://19.3.8
krustlet      Ready    agent    34s     v1.17.0   172.26.47.208   <none>        <unknown>        <unknown>                     mvp
```

## Optional: Delete the Krustlet node
Once you will no more need the Krustlet node, you can remove it from your cluster with the following `kubectl delete node` command:

```shell
$ kubectl delete node krustlet
node "krustlet" deleted

$ kubectl get nodes -o wide
NAME             STATUS   ROLES    AGE   VERSION   INTERNAL-IP    EXTERNAL-IP   OS-IMAGE         KERNEL-VERSION                CONTAINER-RUNTIME
docker-desktop   Ready    master   4d    v1.15.5   192.168.65.3   <none>        Docker Desktop   4.19.104-microsoft-standard   docker://19.3.8
```