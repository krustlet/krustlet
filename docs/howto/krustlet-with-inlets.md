# Running Krustlet on Kubernetes with inlets

These are steps for running a krustlet node on your own computer. You can run any kind of Kubernetes
cluster you like, whether that's local on your computer or remote in a managed Kubernetes offering.

The instructions provided in this guide were contributed by [Alex
Ellis](https://github.com/alexellis). For support with the instructions, see the [#inlets channel of
OpenFaaS Slack](https://slack.openfaas.io/) or raise a GitHub issue and tag `@alexellis`.

## Prerequisites

There are a number of ways to give the Kubernetes API server access to the krustlet's API. Various
methods include using a VPN, a VM within the Kubernetes cluster's private network, or a tunnel.
[Inlets](https://docs.inlets.dev/) is a popular open source service tunnel and proxy. It can be used
to forward the port of the krustlet to the Kubernetes cluster so that the Kubernetes API server can
access it as if it were deployed within the cluster directly.

The tunnel has two components: A client which runs on your local machine, and a server which is
deployed as a Pod inside the Kubernetes cluster. The client connects to the server and provides a
persistent link.

Download the latest release of the inlets binary from the [project release
page](https://github.com/inlets/inlets/releases).

Move the binary to `/usr/local/bin`, or place it somewhere on your `$PATH`.

## Step 1: Get a bootstrap config

Krustlet requires a bootstrap token and config the first time it runs. Follow the guide
[here](bootstrapping.md) to generate a bootstrap config and then return to this document. This will
If you already have a kubeconfig available that you generated through another process, you can
proceed to the next step. However, the credentials Krustlet uses must be part of the `system:nodes`
group in order for things to function properly.

## Step 2: Create the inlets service

In order to start Krustlet with the correct node IP address, you'll need to create the `inlets`
service in Kubernetes like so:

```shell
cat <<EOF | kubectl apply -f -                            
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
EOF
```

Once it has been created, run the following command to have the node IP available for next steps.
This is a stable IP and won't change.

```shell
export NODE_IP=$(kubectl get service inlets -o jsonpath="{.spec.clusterIP}")
```

## Step 3: Run `krustlet`

You'll need the certificates generated from the bootstrap process for our next steps, so go ahead
and start krustlet:

```shell
# Since you are running locally, this step is important. Otherwise krustlet will pick up on your
# local config and not be able to update the node status properly
export KUBECONFIG=~/.krustlet/config/kubeconfig
krustlet-wasi --node-ip $NODE_IP --cert-file=~/.krustlet/config/krustlet.crt --private-key-file=~/.krustlet/config/krustlet.key --bootstrap-file=~/.krustlet/config/bootstrap.conf
```

Then open another terminal for the next steps.

## Step 4: Setup inlets server

Create a Kubernetes secret for the inlets server:

```shell
export TOKEN=$(head -c 16 /dev/urandom |shasum|cut -d- -f1)
echo $TOKEN > token.txt

kubectl create secret generic inlets-token --from-literal token=${TOKEN}
```

Then, create a Kubernetes secret for krustlet's TLS certificates. These will be used by the inlets
server so that the kubelet can access the tunnel using the expected TLS certificates.

```shell
kubectl create secret ghosttunnel-tls generic \
  --from-file tls.crt=~/.krustlet/config/krustlet.crt \
  --from-file tls.key=~/.krustlet/config/krustlet.key
```

The inlets OSS version exposes services with HTTP within the cluster, so this example uses
`ghosttunnel` as a tiny reverse proxy to mount the krustlet's TLS certificates so that the kubelet
gets a valid HTTPS response. The service created before will expose it to the cluster

```yaml
$ cat <<EOF | kubectl apply -f -
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

## Step 5: Run the inlets client

Port-forward or expose the inlets server with:

```shell
kubectl port-forward svc/inlets 8000:8000 &
```

You can also expose inlets via Ingress using cert-manager to give its control-port a TLS
certificate.

Run the `inlets client` on your computer:

```shell
inlets client \
  --upstream https://127.0.0.1:3000 \
  --remote ws://127.0.0.1:8000 --token $(token.txt)
```


## Step 6: Verify the node is available

Show that the krustlet node has joined the cluster:

```shell
$ kubectl get nodes -o wide
NAME                   STATUS   ROLES    AGE    VERSION   INTERNAL-IP      EXTERNAL-IP       OS-IMAGE                       KERNEL-VERSION         CONTAINER-RUNTIME
pool-3xbltttyc-3no2p   Ready    <none>   153m   v1.16.6   10.131.25.141    206.189.19.185    Debian GNU/Linux 9 (stretch)   4.19.0-0.bpo.6-amd64   docker://18.9.2
pool-3xbltttyc-3no2s   Ready    <none>   153m   v1.16.6   10.131.28.223    206.189.123.184   Debian GNU/Linux 9 (stretch)   4.19.0-0.bpo.6-amd64   docker://18.9.2
krustlet               Ready    agent    43m    v1.17.0   10.245.157.226   <none>            <unknown>                      <unknown>              mvp
```

## Appendix

### Remove the port-forward for [inlets OSS](https://docs.inlets.dev)

We are using a port-forward to make it easier to use the tutorial. For permanent use, you will want
to expose the inlets server and its control port directly. The OSS version can be configured with
TLS, but this is not built-in.

You can set up an Ingress rule for the control-port of the inlets server (port 8000), and obtain a
TLS certificate from LetsEncrypt.

### Use inlets PRO instead

Inlets OSS is an L7 proxy that requires additional work to configure for krustlet. inlets PRO is a
pure L4 TCP proxy with built-in TLS for the control-plane.

With [inlets PRO](https://github.com/inlets/inlets-pro) you can expose the control port (8123)
directly to the Internet as a NodePort, or LoadBalancer, or if you wish via an Ingress definition.
The control port already has TLS configured, so won't need additional link-layer encryption.
