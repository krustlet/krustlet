# Krustlet on Azure

[![Deploy To Azure](https://aka.ms/deploytoazurebutton)](https://portal.azure.com/#create/Microsoft.Template/uri/https%3A%2F%2Fraw.githubusercontent.com%2Fdeislabs%2Fkrustlet%2Fmaster%2Fcontrib%2Fazure%2Fazuredeploy.json)

This template deploys and sets up a customized Krustlet instance on an Ubuntu
Virtual Machine. It also deploys a Virtual Network and a Kubernetes cluster
(via AKS).

You can set common Krustlet server properties as parameters at deployment time.

Once the deployment is successful, you can connect to the Kubernetes cluster
using `az aks get-credentials`.

## Prerequisites

Prior to deploying AKS using this ARM template, a Service Principal needs to
exist.

The following Azure CLI command can be used to create a Service Principal:

```console
$ az ad sp create-for-rbac --name ServicePrincipalName --skip-assignment
```

The output for a service principal with password authentication includes the
password key. Make sure you copy this value - it can't be retrieved. If you
forget the password,
[reset the service principal credentials](https://docs.microsoft.com/en-us/cli/azure/create-an-azure-service-principal-azure-cli#reset-credentials).

The ID and password appears in the output of `az ad sp create-for-rbac` and are
used in the ARM template's parameters. Make sure to record their values for
later use.

You will also need to generate an SSH key. This will be used to SSH into the
machines for debugging purposes... Or if you're just curious and want to see
[how the sausage is made](https://en.wiktionary.org/wiki/how_the_sausage_gets_made).

Copy the content of your public key and paste it into the "SSH Public Key"
parameter.
