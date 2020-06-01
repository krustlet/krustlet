# TLS Bootstraping Instructions

[TLS bootstrapping](https://kubernetes.io/docs/reference/command-line-tools-reference/kubelet-tls-bootstrapping/), at a high level, requires the generation of a bootstrap-kubelet.conf file that contains a bootstrap token.
A bootstrap token is a token that has just enough permissions to allow kubelets too auto-negotiate
mTLS configuration with the Kube API server. From the Krustlet perspective, this will require developers/operators
to execute the following steps:

- Generate a bootstrap-kubelet.conf file containing a bootstrap token
- Proxy the Kube API server to the host
- Modify the bootstrap-kubelet.conf file to reflect host to cluster proxy configuration
- Set required kubernetes environment variables on the host
- Remove/archive all pem/key/crt files in the kubeconfig directory that may have been created when installing krustlet
- Start the krustlet
- Approve the CSR

## Generate a bootstrap-kubelet.conf file containing a bootstrap token

To generate a bootstrap config you must execute a set of commands from a node
in your cluster that has the role `master` assigned.

To identify your nodes execute this command.

```shell
$ kubectl get nodes
```

The output should look similar to this.

```shell
NAME                     STATUS     ROLES    AGE   VERSION
krustlet-control-plane   Ready      master   8d    v1.17.0
```

Look for a node that has ROLES - master. Exceute this command to open an
interactive terminal on a master node.

```shell
$ docker exec -it krustlet-control-plane /bin/bash
```

Once connected to a master node, you will need to execute the following set of commands
to generate a bootstrap-kubelet.conf file on the master node with a bootstrap token
embedded. NOTE: Path's will vary based on how you are running your cluster. The paths below
match a local cluster running with KinD.

```shell
API_SERVER="https://$(cat /etc/kubernetes/kubelet.conf | grep server | sed -r 's/[^0-9.]+/\n/' | xargs)" && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig set-cluster kubernetes --server=$API_SERVER --certificate-authority=/etc/kubernetes/pki/ca.crt --embed-certs=true &&
BOOTSTRAP_TOKEN=$(kubeadm token generate) && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig set-credentials tls-bootstrap-token-user --token=$BOOTSTRAP_TOKEN && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig set-context tls-bootstrap-token-user@kubernetes --user=tls-bootstrap-token-user --cluster=kubernetes && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig use-context tls-bootstrap-token-user@kubernetes
```

This will generate a file call bootstrap-kubeconfig in the /etc/kubernetes/ directory.
Again, if you are not using KinD, the path may be different. See example-boostrap-kubelet.conf
in this directory for a example output.

Execute the following command to view the contents of the generated file on the master node.

```shell
$ cat /etc/kubernetes/bootstrap-kubeconfig

apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: <redacted>
    server: https://172.17.0.2:6443
  name: kubernetes
contexts:
- context:
    cluster: kubernetes
    user: tls-bootstrap-token-user
  name: tls-bootstrap-token-user@kubernetes
current-context: tls-bootstrap-token-user@kubernetes
kind: Config
preferences: {}
users:
- name: tls-bootstrap-token-user
  user:
    token: <redacted>
```

Copy the contents of this file from the master node and save it on the host machine
as `/etc/kubernetes/bootstrap-kubelet.conf`.

## Proxy the Kube API server to the host

Expose the Kube API server to the host via proxy with the following command: `kubectl proxy --port=6443`.
You may choose any unassigned port. At least KinD seems to default to 6443 for
in cluster communication, so that port is used throughout these examples.

## Modify the bootstrap-kubelet.conf file to reflect host to cluster proxy configuration

The cluster stanza in the file you just created will contain an entry for the server
address that looks like this `server: https://172.17.0.2:6443`. This is the cluster
internal address and likely will not be reachable from your krustlet running on
the host. Depending on your kubernetes provider and network configuration the ip
address/port may vary. Modify the bootstrap-kubelet.conf server entry so that it
interacts with the proxy e.g. `server: http://127.0.0.1:6443`. NOTE: https is not
supported. You must send the request over http.

## Set required kubernetes environment variables on the host

You will need to set 3 environment variables. KUBERNETS_SERVICE_HOST and
KUBERNETES_SERVICE_PORT reflect the localhost:port proxy configuration.

- `export KUBERNETES_SERVICE_HOST="127.0.0.1"`
- `export KUBERNETES_SERVICE_PORT="6443"`

## Remove/archive all files in the ~/.krustlet/config directory

If you have followed the krustlet installation instructions in other documentation
you already have files in your ~/.krustlet/config directory. If you wish to you use
them again, move them to another location now. Otherwise, simply delete them. The
bootstrapping processing will generate 4 files in that directory:

- config (no extension)
- certificate.pfx
- host.key
- host.cert

## Start the krustlet

The following command will start the krustlet. The bootstrapping process will detect
that no config file is present, and initiate the bootstrap by sending a Certificate
Signing Request to the Kube API server with the bootstrap token from the .conf file
as a bearer token in the Authorization header. NOTE: the Kube API server proxy must
still be running.

```shell
just run-wasi
```
