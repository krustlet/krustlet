# Running Krustlet on Kubernetes with inlets

These are steps for running a krustlet node on your own computer. You can run any kind of 
Kubernetes cluster you like, whether that's local on your computer or remote in a managed 
Kubernetes offering.

## Prerequisites

There are a number of ways to give the Kubernetes API server access to the krustlet's API. Various methods include using a VPN, a VM within the Kubernetes cluster's private network, or a tunnel. Inlets is a popular open source service tunnel and proxy that is listed on the CNCF Landscape. It can be used to forward the port of the krustlet to the Kubernetes cluster so that the API server can access it as if it were deployed within the cluster directly.

The tunnel has two components. A client which runs on your local machine, and a server which is deployed as a Pod inside the Kubernetes cluster. The client connects to the server and provides a persistent link.

* [inlets - "The Cloud Native Tunnel"](https://docs.inlets.dev/)

Run without `sudo` to download the binary to your local directory, then move it to your `PATH`.

```shell
curl -sLS https://get.inlets.dev | sh
chmod +x inlets
sudo mv inlets /usr/local/bin/
```

## Step 1: Create a service account user for the node

We will need to create a Kubernetes configuration file (known as the kubeconfig) and service account
for krustlet to use to register nodes and access specific secrets.

This can be done by using the Kubernetes manifest in the [assets](./assets) directory:

```shell
$ kubectl apply -n kube-system -f ./docs/howto/assets/krustlet-service-account.yaml
```

You can also do this by using the manifest straight from GitHub:

```shell
$ kubectl apply -n kube-system -f https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/krustlet-service-account.yaml
```

Now that things are all set up, we need to generate the kubeconfig. You can do this by running
(assuming you are in the root of the krustlet repo):

```shell
$ ./docs/howto/assets/generate-kubeconfig.sh
```

Or if you are feeling a bit more trusting, you can run it straight from the repo:

```shell
bash <(curl https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/generate-kubeconfig.sh)
```

Either way, it will output a file called `kubeconfig-sa` in your current directory. Save this for
later.

## Step 2: Create Certificate

Krustlet requires a certificate for securing communication with the Kubernetes API. Because
Kubernetes has its own certificates, we'll need to get a signed certificate from the Kubernetes API
that we can use. First things first, let's create a certificate signing request (CSR):

```shell
$ openssl req -new -sha256 -newkey rsa:2048 -keyout krustlet.key -out krustlet.csr -nodes -subj "/C=US/ST=./L=./O=./OU=./CN=krustlet"
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
$ kubectl get csr krustlet -o jsonpath='{.status.certificate}' \
    | base64 --decode > krustlet.crt
```

Lastly, combine the key and the cert into a PFX bundle, choosing your own password instead of
"password":

```shell
$ openssl pkcs12 -export -out krustlet.pfx -inkey krustlet.key -in krustlet.crt -password "pass:password"
```

## Step 3: Setup inlets server

Create a Kubernetes secret for the inlets server

```shell
export TOKEN=$(head -c 16 /dev/urandom |shasum|cut -d- -f1)
echo $TOKEN > token.txt

kubectl create secret generic inlets-token --from-literal token=${TOKEN}
```

Create a Kubernetes secret for krustlet's TLS certificates

These will be used by the inlets server so that the kubelet can access the tunnel using 
the expected TLS certificates.

```shell
kubectl create secret ghosttunnel-tls generic \
  --from-file tls.crt=krustlet.crt \
  --from-file tls.key=krustlet.key
```

Apply the inlets server Deployment and Service

The inlets OSS version exposes services with HTTP within the cluster, so this example 
uses `ghosttunnel` as a tiny reverse proxy to mount the krustlet's TLS certificates so 
that the kubelet gets a valid HTTPS response.

```yaml
$ cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: Service
metadata:
  name: inlets
  labels:
    app: inlets
spec:
  type: ClusterIP
  ports:
    - port: 8000
      protocol: TCP
      targetPort: 8000
      name: control
    - port: 3000
      protocol: TCP
      targetPort: 3000
      name: data
  selector:
    app: inlets
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: inlets
spec:
  replicas: 1
  selector:
    matchLabels:
      app: inlets
  template:
    metadata:
      labels:
        app: inlets
    spec:
      volumes:
        - name: ghosttunnel-tls-volume
          secret:
            secretName: ghosttunnel-tls
        - name: inlets-token-volume
          secret:
            secretName: inlets-token
      containers:
      - name: inlets
        image: inlets/inlets:2.7.0
        imagePullPolicy: Always
        command: ["inlets"]
        args:
        - "server"
        - "--token-from=/var/inlets/token"
        - "--control-port=8000"
        - "--port=3001"
        volumeMounts:
          - name: inlets-token-volume
            mountPath: /var/inlets/
      - name: ghosttunnel
        image: squareup/ghostunnel:v1.5.2
        imagePullPolicy: Always
        args:
        - "server"
        - "--target=127.0.0.1:3001"
        - "--listen=0.0.0.0:3000"
        - "--cert=/etc/tls/tls.crt"
        - "--key=/etc/tls/tls.key"
        - "--disable-authentication"
        volumeMounts:
          - name: ghosttunnel-tls-volume
            mountPath: /etc/tls
EOF
```

## Step 4: Run the inlets client

Port-forward or expose the inlets server

```shell
kubectl port-forward svc/inlets 8000:8000 &
```

You can also expose inlets via Ingress using cert-manager to give its control-port a TLS certificate

Run the `inlets client` on your computer

```shell
inlets client \
  --upstream https://127.0.0.1:3000 \
  --remote ws://127.0.0.1:8000 --token $(token.txt)
```

Get the inlets server's service IP, this is a stable IP and won't change.

```shell
export NODE_IP=$(kubectl get service inlets -o jsonpath="{.spec.clusterIP}")
```

## Step 5: Run the `krustlet` and verify the node is available

```shell
krustlet-wasi --node-ip $NODE_IP --pfx-password password
```

* Show that the krustlet node has joined the cluster

```shell
$ kubectl get nodes -o wide
NAME                   STATUS   ROLES    AGE    VERSION   INTERNAL-IP      EXTERNAL-IP       OS-IMAGE                       KERNEL-VERSION         CONTAINER-RUNTIME
pool-3xbltttyc-3no2p   Ready    <none>   153m   v1.16.6   10.131.25.141    206.189.19.185    Debian GNU/Linux 9 (stretch)   4.19.0-0.bpo.6-amd64   docker://18.9.2
pool-3xbltttyc-3no2s   Ready    <none>   153m   v1.16.6   10.131.28.223    206.189.123.184   Debian GNU/Linux 9 (stretch)   4.19.0-0.bpo.6-amd64   docker://18.9.2
krustlet               Ready    agent    43m    v1.17.0   10.245.157.226   <none>            <unknown>                      <unknown>              mvp
```

You can now go on to test that things work by applying the sample manifest and checking for its logs.

## Appendix

The instructions were contributed by [Alex Ellis](https://github.com/alexellis), for support with the instructions, see the [#inlets channel of OpenFaaS Slack](https://slack.openfaas.io/) or raise a GitHub issue and tag `@alexellis`.

### Remove the port-forward for [inlets OSS](https://docs.inlets.dev)

We are using a port-forward to make it easier to use the tutorial. For permanent use, you will want to expose the inlets server and its control port directly. The OSS version can be configured with TLS, but this is not built-in.

You can set up an Ingress rule for the control-port of the inlets server (port 8000), and obtain a TLS certificate from LetsEncrypt.

### Use inlets PRO instead

Inlets OSS is an L7 proxy that requires additional work to configure for krustlet. inlets PRO is a pure L4 TCP proxy with built-in TLS for the control-plane.

With [inlets PRO](https://github.com/inlets/inlets-pro) you can expose the control port (8123) directly to the Internet as a NodePort, or LoadBalancer, or if you wish via an Ingress definition. The control port already has TLS configured, so won't need additional link-layer encryption.
