# TLS Bootstraping Instructions

[TLS bootstrapping](https://kubernetes.io/docs/reference/command-line-tools-reference/kubelet-tls-bootstrapping/), at a high level, requires the generation of a bootstrap-kubelet.conf file that contains a bootstrap token.
A bootstrap token is a token that has just enough permissions to allow kubelets too auto-negotiate
mTLS configuration with the Kube API server. From the Krustlet perspective, this will require developers/operators
to execute the following steps:

- Generate a bootstrap-kubelet.conf file containing a boostrap token
- Proxy the Kube API server to the host
- Modify the bootstrap-kubelet.conf file to reflect host to cluster proxy configuration
- Set required kubernetes environment variables on the host
- Remove/archive all pem/key/crt files in the kubeconfig directory that may have been created when installing krustlet
-

To generate a bootstrap config you must execute the following commands from the master node
in your cluster. Path's will vary based on how you are running your cluster. The paths below
work on KinD.

To identify your nodes execute this command.

```shell
$ kubectl get nodes
```

The output should look similar to this.

```shell
NAME                     STATUS     ROLES    AGE   VERSION
krustlet-control-plane   Ready      master   8d    v1.17.0
```

Exceute this command to open an interactive termional with a master node.

```shell
$ docker exec -it krustlet-control-plane /bin/bash
```

```shell
API_SERVER="https://$(cat /etc/kubernetes/kubelet.conf | grep server | sed -r 's/[^0-9.]+/\n/' | xargs)" && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig set-cluster kubernetes --server=$API_SERVER --certificate-authority=/etc/kubernetes/pki/ca.crt --embed-certs=true &&
BOOTSTRAP_TOKEN=$(kubeadm token generate) && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig set-credentials tls-bootstrap-token-user --token=$BOOTSTRAP_TOKEN && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig set-context tls-bootstrap-token-user@kubernetes --user=tls-bootstrap-token-user --cluster=kubernetes && \
kubectl config --kubeconfig=/etc/kubernetes/bootstrap-kubeconfig use-context tls-bootstrap-token-user@kubernetes
```

This will generate a file call bootstrap-kubelet.conf in the specified directory. The cluster stanza
will contain an entry for the server address that looks like this `server: https://172.17.0.2:6443`.
This is the cluster internal address and will not be reachable from your krustlet running on the host.
Depending on your kubernetes provider and networking configuration the ip address/port may vary.

To enable host to cluster communications for the krustlet bootstrapping process you must:

- Expose the Kube API server via proxy with the following command: `kubectl proxy --port=6443`.
  You may choose any unassigned port. KinD seems to default to 6443 for in cluster communication
  so that port is used throughout these examples.
- Copy the contents of the generated bootstrap-kubelet.conf file from the in cluster node where you generated
- Modify the bootstrap-kubelet.conf server entry so that it talks to the proxy. e.g. `server: https://127.0.0.1:6443`.
  See example-boostrap-kubelet.conf in this directory for a example output.
- Set the KUBERNETES_SERVICE_HOST environment variable to localhost. e.g. `export KUBERNETES_SERVICE_HOST="127.0.0.1"`
- Set the KUBERNETES_SERVICE_PORT to the port you chose. e.g. `export KUBERNETES_SERVICE_PORT="6443"`.
- Set the KUBECONFIG to ~/.krustlet/config/config
