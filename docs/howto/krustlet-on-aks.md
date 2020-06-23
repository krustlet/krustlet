# Running Krustlet on Azure Kubernetes Service (AKS)

These are steps for running a Krustlet node in an AKS cluster. Ideally, we will want to simplify
this process in the future so you do not have to do a bunch of configuration to connect a node to
the cluster.

## Prerequisites

You will require a running AKS cluster for this guide. The steps below assume a non-custom vnet,
though they are easily adaptable to custom vnets. `kubectl` is also required, preferrably with "root
access" to the cluster

See the [how-to guide for running Kubernetes on AKS](kubernetes-on-aks.md) for more information.

This specific tutorial will be running Krustlet on another Azure Virtual Machine; however, you can
follow these steps from any device that can start a web server on an IP accessible from the
Kubernetes control plane.

## Step 1: Creating and accessing a new VM

Now we need to create a VM in the same resource group as the AKS cluster. In order for the
Kubernetes master to be able to access the node, we'll need to do some additional legwork. The node
will need to be in the same vnet, subnet, and resource group as the nodes in the cluster. When you
create an AKS cluster, it actually creates the resources in a separate resource group so they can be
managed properly.

To find the resource group of your cluster and export it as a variable for use in other steps, run
the following command:

```shell
$ export CLUSTER_RESOURCE_GROUP=$(az aks show --resource-group <resource group of AKS cluster> --name <AKS cluster name> --query nodeResourceGroup -o tsv)
```

You'll also need the name of the vnet in that resource group:

```shell
$ az network vnet list --resource-group $CLUSTER_RESOURCE_GROUP -o table
```

Copy the name of the vnet from the output and then run the following command. This command will
create a new small sized (you can change the size if you desire) Ubuntu VM in the right vnet and
using the `aks-subnet` inside of the vnet. It will also create a user named `krustlet` with SSH
authentication using your public key. If you do not have SSH keys configured, see [this useful
guide](https://help.github.com/en/github/authenticating-to-github/generating-a-new-ssh-key-and-adding-it-to-the-ssh-agent)
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

Because the new node is in the AKS vnet, you cannot access it directly. In order to access it,
you'll need to be in a pod in the cluster. To do so, follow the step in the documentation [found
here](https://docs.microsoft.com/en-us/azure/aks/ssh) and return to this document after you have
copied your SSH key into the pod.

Once you have the SSH key in the pod, you should be able to access the node by running:

```shell
$ ssh -i id_rsa krustlet@<private IP from above>
```

## Step 2: Configure AKS for bootstrapping

By default, AKS doesn't have automatic bootstrapping enabled. To do so, you'll need to create the
following role bindings from a terminal on your machine:

```shell
$ cat <<EOF | kubectl apply --filename=-
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: kubelet-bootstrap
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:node-bootstrapper
subjects:
- apiGroup: rbac.authorization.k8s.io
  kind: Group
  name: system:bootstrappers
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: node-autoapprove-bootstrap
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:certificates.k8s.io:certificatesigningrequests:nodeclient
subjects:
- apiGroup: rbac.authorization.k8s.io
  kind: Group
  name: system:bootstrappers
EOF
```

This will create the right bindings to preexisting groups so certs get approved properly

## Step 3: Install and configure Krustlet

First we'll create the config directory for Krustlet:

```shell
$ sudo mkdir -p /etc/krustlet/config && sudo chown -R krustlet:krustlet /etc/krustlet
```

Once that is in place, we'll install the latest release of Krustlet following [the install
guide](../intro/install.md).

Once Krustlet is installed, we need to create a systemd unit file that we'll enable in a later step.
There is an example [`krustlet.service`](./assets/krustlet.service) that you can either copy to the
box, but it is probably easier just to copy and paste it to `/etc/systemd/system/krustlet.service`
on the VM, changing any environment variables to your own configuration if desired

## Step 4: Get a bootstrap config for your Krustlet node

Krustlet requires a bootstrap token and config the first time it runs. In another terminal on your
machine, follow the guide [here](bootstrapping.md), setting the `CONFIG_DIR` variable to `./`, to
generate a bootstrap config and then return to this document. If you already have a kubeconfig
available that you generated through another process, you can proceed to the next step. However, the
credentials Krustlet uses must be part of the `system:nodes` group in order for things to function
properly.

NOTE: You may be wondering why you can't run this on the VM you just provisioned. We need access to
the Kubernetes API in order to create the bootstrap token, so the script used to generate the
bootstrap config needs to be run on a machine with the proper Kubernetes credentials

Once you have generated the boostrap config, copy it up to the ssh container like so:

```shell
$ kubectl cp bootstrap.conf $(kubectl get pod -l run=aks-ssh -o jsonpath='{.items[0].metadata.name}'):/
```

Now return to your terminal in the pod and log out from the node. Then, copy the bootstrap conf to the new VM and ssh to the VM again:

```shell
$ scp -i id_rsa bootstrap.conf krustlet@<private IP from above>:/etc/krustlet/config/
$ ssh -i id_rsa krustlet@<private IP from above>
```

## Step 5: Start the Krustlet service

Once you have your bootstrap config in place, run the following commands to make sure the unit is configured to start on
boot:

```shell
$ sudo systemctl enable krustlet && sudo systemctl start krustlet
```

### Step 5a: Approving the serving CSR

Once you have started Krustlet, there is one more manual step (though this could be automated
depending on your setup) to perform. The client certs Krustlet needs are generally approved
automatically by the API. However, the serving certs require manual approval. To do this, you'll
need the hostname you specified for the `--hostname` flag or the output of `hostname` if you didn't
specify anything. From the terminal on your machine, run:

```bash
$ kubectl certificate approve <hostname>-tls
```

NOTE: You will only need to do this approval step the first time Krustlet starts. It will generate
and save all of the needed credentials to your machine

Once you do this, Krustlet will automatically grab the new certs and start running. To confirm this,
in another terminal, run `kubectl get nodes -o wide` and you should see output that looks similar to
below:

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
