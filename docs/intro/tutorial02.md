# Writing your first app, part 2

This tutorial begins where [Tutorial 1](tutorial01.md) left off. We’ll walk through the process to set up your personal
registry and publish your application to that registry.

For this tutorial, we will be creating a registry hosted on Microsoft Azure, but there are other cloud providers that
provide their own solutions, and you can [run one on your own infrastructure](https://github.com/docker/distribution),
too!

## What is a registry, and what is wasm-to-oci?

A registry allows you to store your local WebAssembly modules in the cloud. With a registry, you can backup your
personal modules, share your projects, and collaborate with others.

[wasm-to-oci][] is an open source project that understands how to communicate with a registry. It takes a module you've
built locally on your computer and publishes it to the registry, making it publicly available for others to access.

## Create a registry

This tutorial uses the Azure CLI to create an Azure Container Registry. We will be using this registry to publish our
modules and provide Krustlet the URL for fetching these modules.

The steps here assume you have an Azure account and the `az` CLI installed. However, there are other cloud providers
available with their own solutions, and if you're feeling particularly brave, you can
[run your own registry on your own infrastructure](https://github.com/docker/distribution).

### Create a resource group

An Azure resource group is a logical container into which Azure resources are deployed and managed.

The following example creates a resource group named `myResourceGroup` in the `eastus` region. You may want to change it
to a region closer to you. You can find out what regions are available with `az account list-locations`, and you can set
your default region with `az configure --defaults location=<location>`.

Create a resource group with the `az group create` command.

```console
$ az group create --name myResourceGroup --location eastus
```

### Create a container registry

In this tutorial, we will be creating a basic registry, which is cost-optimized for developers learning about Azure
Container Registry. For details on available service tiers, see
[Container registry SKUs](https://docs.microsoft.com/en-us/azure/container-registry/container-registry-skus) in the
Azure documentation.

Create an ACR instance using the `az acr create command`. The registry name must be unique within Azure, and contain
5-50 alphanumeric characters.

In the following example, `mycontainerregistry007` is used as the name. Update this to a unique value.

```console
$ az acr create --sku Basic --resource-group myResourceGroup --name mycontainerregistry007
```

When the registry is created, the output is similar to the following:

```json
{
  "adminUserEnabled": false,
  "creationDate": "2019-01-08T22:32:13.175925+00:00",
  "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/myResourceGroup/providers/Microsoft.ContainerRegistry/registries/mycontainerregistry007",
  "location": "eastus",
  "loginServer": "mycontainerregistry007.azurecr.io",
  "name": "mycontainerregistry007",
  "provisioningState": "Succeeded",
  "resourceGroup": "myResourceGroup",
  "sku": {
    "name": "Basic",
    "tier": "Basic"
  },
  "status": null,
  "storageAccount": null,
  "tags": {},
  "type": "Microsoft.ContainerRegistry/registries"
}
```

Take note of the `loginServer` field. That is the URL for our registry. We'll need to know that when we publish our
application in a bit.

### Log in

Now that our registry was created, we can go ahead and authenticate with this registry to publish our application:

```console
$ az acr login --name mycontainerregistry007
```

## Publish your app

Now that we've created our registry and are logged in, we can publish our application using [wasm-to-oci][].

wasm-to-oci is a tool for publishing WebAssembly modules to a registry. It packages the module and uploads it to the
registry. Krustlet understands the registry API and will fetch the module based on the URL you uploaded it to.

To publish our application, we need to come up with a name and a version number. Our `loginServer` field from earlier
was `mycontainerregistry007.azurecr.io`, and we want to name our application `krustlet-tutorial`, version `v1.0.0`.

The pattern for a registry URL is:

```
<URL>/<NAME>:<VERSION>
```

In our case, our application URL will look like `mycontainerregistry007.azurecr.io/krustlet-tutorial:v1.0.0`. Great!

Let's publish that now:

```console
$ wasm-to-oci push demo.wasm mycontainerregistry007.azurecr.io/krustlet-tutorial:v1.0.0
```

`demo.wasm` is the filename of the WebAssembly module we compiled during [part 1](tutorial01.md) of this tutorial. If
you are publishing the Rust example, use `target/wasm32-wasi/debug/demo.wasm` instead.

When you’re comfortable with publishing your application with wasm-to-oci, read [part 3 of this tutorial](tutorial03.md)
to learn about running a Kubernetes cluster for development purposes. You will also learn how to install Krustlet on
that cluster.


[wasm-to-oci]: https://github.com/engineerd/wasm-to-oci
