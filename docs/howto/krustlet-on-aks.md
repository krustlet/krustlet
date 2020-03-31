# Running Krustlet on Azure Kubernetes Service (AKS)

These are steps for running a krustlet node in an AKS cluster. Ideally, we will want to simplify this process in the
future so you do not have to do a bunch of configuration to connect a node to the cluster.

## Prerequisites

You will require a running AKS cluster for this guide. The steps below assume a non-custom vnet, though they are easily
adaptable to custom vnets. `kubectl` is also required.

See the [how-to guide for running Kubernetes on AKS](kubernetes-on-aks.md) for more information.

This specific tutorial will be running krustlet on another Azure Virtual Machine; however, you can follow these steps
from any device that can start a web server on an IP accessible from the Kubernetes control plane.

## Step 1: Create a service account user for the node

We will need to create a Kubernetes configuration file (known as the kubeconfig) and service account for krustlet to use
to register nodes and access specific secrets.

This can be done by using the Kubernetes manifest in the [assets](./assets) directory:

```shell
$ kubectl apply -n kube-system -f ./docs/howto/assets/krustlet-service-account.yaml
```

You can also do this by using the manifest straight from GitHub:

```shell
$ kubectl apply -n kube-system -f https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/krustlet-service-account.yaml
```

Now that things are all set up, we need to generate the kubeconfig. You can do this by running (assuming you are in the
root of the krustlet repo):

```shell
$ ./docs/howto/assets/generate-kubeconfig.sh
```

Or if you are feeling a bit more trusting, you can run it straight from the repo:

```shell
bash <(curl https://raw.githubusercontent.com/deislabs/krustlet/master/docs/howto/assets/generate-kubeconfig.sh)
```

Either way, it will output a file called `kubeconfig-sa` in your current directory. Save this for later.

## Step 2: Create Certificate

Krustlet requires a certificate for securing communication with the Kubernetes API. Because Kubernetes has its own
certificates, we'll need to get a signed certificate from the Kubernetes API that we can use. First things first, let's
create a certificate signing request (CSR):

```shell
$ openssl req -new -sha256 -newkey rsa:2048 -keyout krustlet.key -out krustlet.csr -nodes -subj "/C=US/ST=./L=./O=./OU=./CN=krustlet"
```

This will create a CSR and a new key for the certificate, using `krustlet` as the hostname of the server.

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

Once that runs, an admin (that is probably you! at least it should be if you are trying to add a node to the cluster)
needs to approve the request:

```shell
$ kubectl certificate approve krustlet
certificatesigningrequest.certificates.k8s.io/krustlet approved
```

After approval, you can download the cert like so:

```shell
$ kubectl get csr krustlet -o jsonpath='{.status.certificate}' \
    | base64 --decode > krustlet.crt
```

Lastly, combine the key and the cert into a PFX bundle, choosing your own password instead of "password":

```shell
$ openssl pkcs12 -export -out krustlet.pfx -inkey krustlet.key -in krustlet.crt -password "pass:password"
```

## Step 3: Creating and accessing a new VM

Now we need to create a VM in the same resource group as the AKS cluster. In order for the Kubernetes master to be able
to access the node, we'll need to do some additional legwork. The node will need to be in the same vnet, subnet, and
resource group as the nodes in the cluster. When you create an AKS cluster, it actually creates the resources in a
separate resource group so they can be managed properly.

To find the resource group of your cluster and export it as a variable for use in other steps, run the following
command:

```shell
$ export CLUSTER_RESOURCE_GROUP=$(az aks show --resource-group <resource group of AKS cluster> --name <AKS cluster name> --query nodeResourceGroup -o tsv)
```

You'll also need the name of the vnet in that resource group:

```shell
$ az network vnet list --resource-group $CLUSTER_RESOURCE_GROUP -o table
```

Copy the name of the vnet from the output and then run the following command. This command will create a new small sized
(you can change the size if you desire) Ubuntu VM in the right vnet and using the `aks-subnet` inside of the vnet. It
will also create a user named `krustlet` with SSH authentication using your public key. If you do not have SSH keys
configured, see [this useful guide](https://help.github.com/en/github/authenticating-to-github/generating-a-new-ssh-key-and-adding-it-to-the-ssh-agent)
for steps on how to do so:

```shell
$ az vm create -n krustlet -g $CLUSTER_RESOURCE_GROUP --image UbuntuLTS --admin-username krustlet --ssh-key-values ~/.ssh/id_rsa.pub --size Standard_B1ms --vnet-name <vnet name from above> --subnet aks-subnet
```

Once the command completes, you'll see output like this:

```json
{
  "fqdns": "",
  "id": "/subscriptions/27f15ea3-ac5d-2fd4-66ca-a5fe9431f5db/resourceGroups/MC_wasm-testing_krustlet-demo_southcentralus/providers/Microsoft.Compute/virtualMachines/krustlet",
  "location": "southcentralus",
  "macAddress": "00-0D-3A-72-2A-92",
  "powerState": "VM running",
  "privateIpAddress": "10.240.0.5",
  "publicIpAddress": "13.65.90.126",
  "resourceGroup": "MC_wasm-testing_krustlet-demo_southcentralus",
  "zones": ""
}
```

You'll need the value of `privateIpAddress` for connecting to the cluster.

Because the new node is in the AKS vnet, you cannot access it directly. In order to access it, you'll need to be in a
pod in the cluster. To do so, follow the step in the documentation
[found here](https://docs.microsoft.com/en-us/azure/aks/ssh) and return to this document after you have copied your SSH
key into the pod.

Once you have the SSH key in the pod, you should be able to access the node by running:

```shell
$ ssh -i id_rsa krustlet@<private IP from above>
```

However, before doing so, go to the next step.

## Step 4: Copy assets to VM

The first thing we'll need to do is copy up the assets we generated in steps 1 and 2. This is a two step process because
we need to copy them to the pod and then copy them to the server. So first open another terminal in the same directory
and then copy them to the pod:

```shell
$ kubectl cp krustlet.pfx $(kubectl get pod -l run=aks-ssh -o jsonpath='{.items[0].metadata.name}'):/
$ kubectl cp kubeconfig-sa $(kubectl get pod -l run=aks-ssh -o jsonpath='{.items[0].metadata.name}'):/
```

Now return to your terminal in the pod and copy them to the new VM:

```shell
$ scp -i id_rsa {krustlet.pfx,kubeconfig-sa} krustlet@<private IP from above>:~/
```

Then go ahead and SSH to the VM:

```shell
$ ssh -i id_rsa krustlet@<private IP from above>
```

## Step 5: Install and configure Krustlet

Whew, ok...that was a lot. Now let's actually configure stuff. First we'll create the config directory for Krustlet and
place all of our assets there:

```shell
$ sudo mkdir -p /etc/krustlet && sudo chown krustlet:krustlet /etc/krustlet
$ mv {krustlet.pfx,kubeconfig-sa} /etc/krustlet && chmod 600 /etc/krustlet/*
```

Once that is in place, we'll download the latest release of krustlet and install it:

<!-- TODO: Add 0.1 link when released -->
```shell
$ curl -LO https://krustlet.blob.core.windows.net/releases/krustlet-canary-Linux-amd64.tar.gz && tar -xzf krustlet-canary-Linux-amd64.tar.gz
$ sudo mv krustlet-wa* /usr/local/bin/
```

Next we'll enable krustlet as a systemd service. There is an example [`krustlet.service`](./assets/krustlet.service)
that you can either copy to the box, but it is probably easier just to copy and paste it to
`/etc/systemd/system/krustlet.service` on the VM. Make sure to change the value of `PFX_PASSWORD` to the password you
set for your certificate.

Once you have done that, run the following commands to make sure the unit is configured to start on boot:

```shell
$ sudo systemctl enable krustlet && sudo systemctl start krustlet
```

In another terminal, run `kubectl get nodes -o wide` and you should see output that looks similar to below:

```
NAME                                STATUS   ROLES   AGE     VERSION   INTERNAL-IP   EXTERNAL-IP   OS-IMAGE             KERNEL-VERSION      CONTAINER-RUNTIME
aks-agentpool-81651327-vmss000000   Ready    agent   3h32m   v1.17.3   10.240.0.4    <none>        Ubuntu 16.04.6 LTS   4.15.0-1071-azure   docker://3.0.10+azure
krustlet                            Ready    agent   11s     v1.17.0   10.240.0.5    <none>        <unknown>            <unknown>           mvp
```

## Step 6: Test that things work

Now you can see things work! Feel free to give any of the demos a try like so:

```shell
$ kubectl apply -f demos/wasi/hello-world-rust/k8s.yaml
# wait a few seconds for the pod to run
$ kubectl logs hello-world-wasi-rust
hello from stdout!
hello from stderr!
CONFIG_MAP_VAL=cool stuff
FOO=bar
POD_NAME=hello-world-wasi-rust
Args are: []
```
