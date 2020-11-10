# Running Krustlet on Azure

This guide demonstrates how to run Krustlet on Azure.

## Prerequisites

This guide will require both [the Azure
CLI](https://docs.microsoft.com/en-us/cli/azure/install-azure-cli) as well as
`kubectl` to connect to the cluster after it has been provisioned.

This specific tutorial will run Krustlet on another Azure Virtual Machine within
the same Virtual Network as the Kubernetes cluster.

## Step 1: Creating a Service Principal

Prior to deploying Krustlet, a Service Principal needs to exist.

The following Azure CLI command can be used to create a Service Principal. Make
sure to change `ServicePrincipalName` with your own unique name for a Service
Principal.

```shell
$ az ad sp create-for-rbac --name ServicePrincipalName --skip-assignment
```

The output for a service principal with password authentication includes the
password key. Make sure you copy this value - it can't be retrieved. If you
forget the password, [reset the service principal
credentials](https://docs.microsoft.com/en-us/cli/azure/create-an-azure-service-principal-azure-cli#reset-credentials).

The ID and password appears in the output of `az ad sp create-for-rbac` and are
used in the ARM template's parameters. Make sure to record their values for
later use.

## Step 2: Generate an SSH Key

You will also need to generate an SSH key. This will be used to SSH into the
machines for debugging purposes... Or if you're just curious and want to see
[how the sausage is
made](https://en.wiktionary.org/wiki/how_the_sausage_gets_made).

Follow the guide on Github to [generate a new SSH
key](https://docs.github.com/en/free-pro-team@latest/github/authenticating-to-github/generating-a-new-ssh-key-and-adding-it-to-the-ssh-agent).

## Step 3: Click the Button

Go ahead. Click the button.

[![Deploy To
Azure](https://aka.ms/deploytoazurebutton)](https://portal.azure.com/#create/Microsoft.Template/uri/https%3A%2F%2Fraw.githubusercontent.com%2Fdeislabs%2Fkrustlet%2Fmaster%2Fcontrib%2Fazure%2Fazuredeploy.json)

Copy the content of your public key and paste it into the "SSH Public Key"
parameter. Also make sure to copy and paste the content of your Service
Principal's client ID (`appID`) and password into the parameters.

## Step 3(b): Troubleshooting

In case the deployment fails, inspect the raw error message Azure reports from
the failed deployment. The deployment could fail due to a name collision, and
the raw error logs could help determine what went wrong.

## Step 4: Test that things work

Once the cluster has been deployed, now you can see things work! Connect to the
cluster and feel free to give any of the demos a try like so:

```shell
$ az aks get-credentials --name krustlet --resource-group my-resource-group
$ kubectl apply -f demos/wasi/hello-world-rust/k8s.yaml
# wait a few seconds for the pod to run
$ kubectl logs hello-world-wasi-rust
hello from stdout!
hello from stderr!
CONFIG_MAP_VAL=cool stuff
FOO=bar
POD_NAME=hello-world-wasi-rust
Args are: []

Bacon ipsum dolor amet chuck turducken porchetta, tri-tip spare ribs t-bone ham hock. Meatloaf
pork belly leberkas, ham beef pig corned beef boudin ground round meatball alcatra jerky.
Pancetta brisket pastrami, flank pork chop ball tip short loin burgdoggen. Tri-tip kevin
shoulder cow andouille. Prosciutto chislic cupim, short ribs venison jerky beef ribs ham hock
short loin fatback. Bresaola meatloaf capicola pancetta, prosciutto chicken landjaeger andouille
swine kielbasa drumstick cupim tenderloin chuck shank. Flank jowl leberkas turducken ham tongue
beef ribs shankle meatloaf drumstick pork t-bone frankfurter tri-tip.
```

## Step 5: Tear it down

After you're done testing, delete the cluster with

```console
$ az group delete -n my-resource-group
```
