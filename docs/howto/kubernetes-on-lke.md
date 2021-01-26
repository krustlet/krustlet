# Running Kubernetes on Linode Kubernetes Engine (LKE)

[Linode Kubernetes Engine (LKE)](https://www.linode.com/products/kubernetes/) is
a fully-managed Kubernetes service.

If you have used Linode before, you'll need an account. Linode offers [$100
credit](https://www.linode.com/lp/brand-free-credit) to new customers.

## Prerequisites

You can create LKE clusters using Linode [console](https://cloud.linode.com) but
the [Linode CLI ](https://www.linode.com/docs/guides/linode-cli/) is good. This
tutorial assumes you're using the CLI and have authenticated using a personal
access token.

## Create LKE cluster

In order to create a cluster, several values must be determined: Kubernetes version,
the Linode region, and the node type and number. You may use the CLI to determine
these values:

```console
$ linode-cli lke versions-list
$ linode-cli regions list
$ linode-cli linodes types
```

Or you may use the following values to create a version 1.18 cluster of one (1)
node using the `g6-standard-1` machine type in Linode's `us-west` region:

```console
$ LABEL=[[YOUR-LABEL]]
$ REGION="us-west"
$ VERSION="1.18"
$ TYPE="g6-standard-1"
$ COUNT="1"

$ linode-cli lke cluster-create \
  --label=${LABEL} \
  --region=${REGION} \
  --k8s_version ${VERSION} \
  --node_pools.type=${TYPE} \
  --node_pools.count=${COUNT}
```

If successful, this will report:

```console
┌───────┬──────────┬─────────┐
│ id    │ label    │ region  │
├───────┼──────────┼─────────┤
│ 12345 │ krustlet │ us-west │
└───────┴──────────┴─────────┘
```

Make a note (`CLUSTER_ID`) of the id of the cluster as you will need this in the next
step.

Unlike other managed Kubernetes solutions, `linode-cli lke cluster-create` does
**not** update your `${KUBECONFIG}` file (Linux: `${HOME}/.kube/config`). To
simplify subsequent steps, add the LKE config produced by the following command
to your default Kubernetes config (Linux: `${HOME}/.kube/config`):

```console
$ linode-cli lke kubeconfig-view ${CLUSTER_ID} --json \
  | jq -r .[].kubeconfig \
  | base64 --decode
```

If successful, you should be able to list the LKE nodes:

```console
$ kubectl get nodes
NAME                          STATUS   ROLES    AGE    VERSION
lke12345-12345-1234567890ab   Ready    <none>   4m1s   v1.18.15
```

## Delete LKE cluster

When you are finished with the cluster, you may delete it:

```console
$ linode-cli lke cluster-delete ${CLUSTER_ID}
```
