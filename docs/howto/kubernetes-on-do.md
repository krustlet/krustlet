# Managed Kubernetes on DigitalOcean

[Managed Kubernetes on DigitalOcean](https://www.digitalocean.com/products/kubernetes/)
is an inexpensive, professionally-managed Kubernetes service.

You'll need a DigitalOcean account and will need to have provided a payment method.
DigitalOcean offers a [free trial](https://try.digitalocean.com/freetrialoffer).

## Prerequisites

You may provision Kubernetes clusters and Droplets (DigitalOcean VMs) using the
[console](https://cloud.digitalocean.com) but DigitalOcean's CLI [doctl](https://github.com/digitalocean/doctl)
is comprehensive and recommended. The instructions that follow assume you've
installed doctl and [authenticated](https://github.com/digitalocean/doctl#authenticating-with-digitalocean)
to a DigitalOcean account.

> **NOTE** If you use the doctl [Snap](https://github.com/digitalocean/doctl#snap-supported-os),
> consider connecting [kubectl](https://github.com/digitalocean/doctl#use-with-kubectl)
> and [ssh-keys](https://github.com/digitalocean/doctl#using-doctl-compute-ssh) to
> simplify the experience.

## Create Managed Kubernetes cluster

`doctl kubernetes` includes commands for provisioning clusters. In order to create
a cluster, you'll need to provide a Kubernetes version, a node instance size, a
DigitalOcean region and the number of nodes. Values for some of the values may
be obtained using the following `doctl kubernetes` commands:

```console
$ doctl kubernetes options versions
$ doctl kubernetes options regions
$ doctl kubernetes options sizes
```

> **NOTE** DigitalOcean uses unique identifiers called "slugs". "slugs" are the
> identifiers used as values in many of `doctl`'s commands, e.g. `1.19.3-do.3` is
> the slug for Kubernetes version `1.19.3`.

If you'd prefer to use some reasonable default values, you may use the following
command to create a cluster in DigitalOcean's San Francisco region, using
Kubernetes `1.19.3` with a single worker node (the master node is free). The
worker node has 1 vCPU and 2GB RAM (currently $10/month).

```console
$ CLUSTER=[[YOUR-CLUSTER-NAME]]
$ VERSION="1.19.3-do.3"
$ SIZE="s-1vcpu-2gb"
$ REGION="sfo3"

$ doctl kubernetes cluster create ${CLUSTER} \
  --auto-upgrade \
  --count 1 \
  --version ${VERSION} \
  --size ${SIZE} \
  --region ${REGION}
```

`doctl kubernetes cluster create` should automatically update your default
Kubernetes config (Linux: `${HOME}/.kube/config`). `doctl kubernetes cluster delete`
will remove this entry when it deletes the cluster. You should be able to:

```console
$ kubectl get nodes
NAME                            STATUS   ROLES    AGE   VERSION
${CLUSTER}-default-pool-39yh5   Ready    <none>   1m    v1.19.3
```

## Delete Managed Kubernetes cluster

When you are finished with the cluster, you may delete it with:

```console
$ doctl kubernetes cluster delete ${CLUSTER}
```

> **NOTE** This command should (!) delete the cluster's entries (context, user)
> from the default Kubernetes config (Linux `{$HOME}/.kube/config) too.

To confirm that the cluster has been deleted, if you try listing the clusters,
the cluster you deleted should no longer be present:

```console
$ doctl kubernetes cluster list
```

Or you may confirm that the Droplets have been deleted:

```console
$ doctl compute droplet list
```
