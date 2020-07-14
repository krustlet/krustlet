# Bootstrapping Krustlet

As of version 0.3.0, Krustlet supports automatic bootstrapping of its authorization and serving
certificates. This document describes how the functionality works.

## Initialization
Krustlet follows the same [initialization
flow](https://kubernetes.io/docs/reference/command-line-tools-reference/kubelet-tls-bootstrapping/#initialization-process)
as Kubelet (with the exception of automatic renewal of certs that are close to expiry).

## Instructions

In order to join a cluster with the proper permissions, Krustlet requires a valid bootstrap config
with a valid bootstrap token. This token can be generated with
[`kubeadm`](https://kubernetes.io/docs/setup/production-environment/tools/kubeadm/install-kubeadm/)
or may already exist depending on your provider. However, in this case, we will be using an easier
method for creating a join token. Either way, the examples here should be useful for figuring out
how to do it differently depending on your setup.

### Prerequisites
You will need `kubectl` [installed](https://kubernetes.io/docs/tasks/tools/install-kubectl/) and a
kubeconfig that has access to create `Secrets` in the `kube-system` namespace and can approve
`CertificateSigningRequests`.

### Generating a token and kubeconfig

We have a useful bootstrapping [bash script](./assets/bootstrap.sh) or [Powershell
script](./assets/bootstrap.sh) that can be used for generating a token and creating a bootstrap
kubeconfig file. If you have cloned the repo, you can run:

```bash
$ ./docs/howto/assets/bootstrap.sh
```

OR

```powershell
$ .\docs\howto\assets\bootstrap.ps1
```

If you are the trusting sort, you can pipe it in from the internet:

```bash
$ bash <(curl https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/bootstrap.sh)
```

OR

```powershell
$ (Invoke-WebRequest -UseBasicParsing https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/bootstrap.ps1).Content | Invoke-Expression
```

This will output a ready-to-use bootstrap config to `$HOME/.krustlet/config/bootstrap.conf`

#### Script configuration
The script also exposes a few configuration options by means of environment variables. These are
detailed in the table below:

| Name         | Description                                                                                                                                                                                         | Default                  |
|--------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|--------------------------|
| `CONFIG_DIR` | The location of your configuration directory for Krustlet. Should be the same as `$KRUSTLET_DATA_DIR/config` where the `KRUSTLET_DATA_DIR` setting is the same one you use for configuring Krustlet | `$HOME/.krustlet/config` |
| `FILE_NAME`  | The name of the file the bootstrap config should be saved to                                                                                                                                        | `bootstrap.conf`         |

#### Nitty-gritty details
This section contains an overview of the nitty-gritty details for those who may be constructing
their own bootstrapping setup. Feel free to skip this section if it doesn't pertain to you.

##### Bootstrap tokens
A bootstrap token has the format of `[a-z0-9]{6}.[a-z0-9]{16}` where the first part is a randomly
generated token id and the second part after the `.` needs to be a cryptographically secure random
string. The token will look something like this: `ke3uxh.vhxb3ttj1nquno5t`. That means you can
generate a token with a simple bash command like so:

```bash
$ echo "$(< /dev/urandom tr -dc a-z0-9 | head -c${1:-6};echo;).$(< /dev/urandom tr -dc a-z0-9 | head -c${1:-16};echo;)"
```

##### Creating the secret
To actually "create" the bootstrap token, it needs to be placed in a `Secret` in the `kube-system`
namespace. The name of the secret should be `bootstrap-token-<token_id>`. Specifically, the secret
should look something like this when you send it to the API:

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: bootstrap-token-<token_id>
  namespace: kube-system
type: bootstrap.kubernetes.io/token
stringData:
  auth-extra-groups: system:bootstrappers:kubeadm:default-node-token
  expiration: 2020-06-04T20:07:24Z
  token-id: <token_id>
  token-secret: <token_secret>
  usage-bootstrap-authentication: "true"
  usage-bootstrap-signing: "true"
```

The main fields you need to set are `token-id`, `token-secret`, and `expiration`.

##### Generating the Kubeconfig
Once you have a `Secret` created, you then have to generate the Kubeconfig. To do so, you'll need
several pieces of information:

- The CA cert for your Kubernetes API. This should be available from the kubeconfig you are
  currently using
- The server hostname or IP address
- The generated bootstrap token

You can either assemble a kubeconfig by hand or use similar steps to what is found in the [bootstrap
script](./assets/bootstrap.sh)

##### An example bootstrap config
This is an example of a bootstrap config file for reference if creating your own workflow

```yaml
apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCk1JSUM1ekNDQWMrZ0F3SUJBZ0lCQVRBTkJna3Foa2lHOXcwQkFRc0ZBREFWTVJNd0VRWURWUVFERXdwdGFXNXAKYTNWaVpVTkJNQjRYRFRFNE1EUXlOREF3TVRVME9Wb1hEVEk0TURReU1UQXdNVFUwT1Zvd0ZURVRNQkVHQTFVRQpBeE1LYldsdWFXdDFZbVZEUVRDQ0FTSXdEUVlKS29aSWh2Y05BUUVCQlFBRGdnRVBBRENDQVFvQ2dnRUJBS1lxCnhzMllBNzRETlFQTVZkbFJ3aXZFWnIwd0lTUHlQZjkzR3ZsUVNKMDQrbFgxSEF3Yi9GM3dqcDVEckVOSDAraTUKbjhZUy9QK3JlNUpqVU9tV1VmMXNtMmVLNHNRNHpNS01kMHc5by9ERlozTHc3K1h6RzMveTdvMkF4SWVlYjBPdgpzbzhwWUVOMklzRUcrRFhpa0l0MjhPZ1RtZGRhTVg1OWJQTXhGL0l6T1FPVmFEYmtnMk5ScWtjYW9CR0FTT2JkCkVId2hrVGdMYXZCNzVnVmRTVVlWUFU1M2dXc3hDQWVBYzNCaW9NekNLNmFFUXIrMDB4V3dEWkR4amxLYU02V3gKTWFQN0JmY0Y5K2U3OUt0Tkc3TXZMWG9xdFJ3cCtPdWREaTlKWHRLS1NNbVo3TFNubEY4UDdHUlhKL2IzNS9NVQpvQklkK1ZKNHpoak5zT2xKM3g4Q0F3RUFBYU5DTUVBd0RnWURWUjBQQVFIL0JBUURBZ0trTUIwR0ExVWRKUVFXCk1CUUdDQ3NHQVFVRkJ3TUNCZ2dyQmdFRkJRY0RBVEFQQmdOVkhSTUJBZjhFQlRBREFRSC9NQTBHQ1NxR1NJYjMKRFFFQkN3VUFBNElCQVFDWmM0SVpST3pnNS91eDdNa0Y3NmVja3dZekY4OUJiejRhRENVS3ByWUMxTDFvZVBVawpXdFc3VENWditDMDJRc2tGRnpTbGhlQUpYeXp5Q2xKMVE5VmUyUmR2bGtiZHVPTXpJeXZTS0xaZHVDT2pvVWNZCjkrMTN1UEFpaXJjNUpRZlBOTGdJcUdhbTB2ZXpqZEtROHNUK0o0WmRyNHdFOUZKQnhOeUhob0xQdjBLRENBbkcKWFlPZW5lTHdjcnJCcTVDTERRN2dIelZGbEFKVU1nSWF3ZzdtcG1HVi9KRlVlYnRpam1Cd1p1WDBNMTFpVHBqYQpNaUhDRkJOREd5a2locDBoSHdDV1ZId0ZXNHVLUWxUZjRBK2hieC9OTUkzbHhBYXozMFZKN3U1Mm1GR3pCQ0dvCmt3VjdKS2RJMEd1MGJQQmlUSDRMTmE4bWxqYmZkRnhsc2k4cAotLS0tLUVORCBDRVJUSUZJQ0FURS0tLS0tCg==
    server: https://192.168.64.19:8443
  name: minikube
contexts:
- context:
    cluster: minikube
    namespace: kube-system
    user: tls-bootstrap-token-user
  name: tls-bootstrap-token-user@kubernetes
current-context: tls-bootstrap-token-user@kubernetes
kind: Config
preferences: {}
users:
- name: tls-bootstrap-token-user
  user:
    token: ke3uxh.vhxb3ttj1nquno5t
```

### Running Krustlet

Once you have the bootstrap config in place, you can run Krustlet:

```bash
$ KUBECONFIG=~/.krustlet/config/kubeconfig krustlet-wasi --port 3000 --bootstrap-file /path/to/your/bootstrap.conf
```

Krustlet will begin the bootstrapping process, and then **await manual certificate approval** (described below) before launching.

A couple important notes here. `KUBECONFIG` should almost always be set, especially in
developer/local machine situations. During the bootstrap process, Krustlet will generate a
kubeconfig with the credentials it obtains during the bootstrapping process and write it out to the
specified location in `KUBECONFIG`. If a kubeconfig already exists there, it will be loaded and skip
the bootstrapping process. A similar process occurs during the bootstrapping of the serving
certificates, they will be written out to the paths specified by `--cert-file` (default
`$KRUSTLET_DATA_DIR/config/krustlet.crt`) and `--private-key-file` (default
`$KRUSTLET_DATA_DIR/config/krustlet.key`). If they already exist, then they will be loaded and
bootstrapping skipped.

### Approving the serving CSR
Once you have started Krustlet, there is one more manual step (though this could be automated
depending on your setup). The client certs Krustlet needs are generally approved automatically by
the API. However, the serving certs require manual approval. To do this, you'll need the hostname
you specified for the `--hostname` flag or the output of `hostname` if you didn't specify anything.
Then run:

```bash
$ kubectl certificate approve <hostname>-tls
```

Once you do this, Krustlet will automatically grab the new certs and start running
