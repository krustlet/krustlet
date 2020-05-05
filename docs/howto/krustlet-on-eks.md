# Running Krustlet on Amazon Elastic Kubernetes Service (EKS)

Currently, [EKS does not support](https://github.com/aws/containers-roadmap/issues/741) running
managed node groups with custom Amazon Machine Images (AMI).

However, it does appear the feature might be coming soon.

Until that time, we can use [eksctl](https://eksctl.io/) to create and manage a node group with a
custom Krustlet-based AMI.

## Prerequisites

The following tools are needed to complete this walkthrough:

* [Amazon CLI](https://aws.amazon.com/cli/) - *use `aws configure` to set your access keys and
  default region*
* [Packer](https://packer.io/)
* [eksctl](https://eksctl.io/)

# Building the Krustlet-based AMI

We will be using [Packer](https://packer.io/) to spin up an EC2 instance to build the AMI.

There is a Makefile in `docs/howto/assets/eks` that will run `packer` for you.  It will use a
`c5.2xlarge` EC2 instance to build the AMI with.  Use the `instance_type` variable to `make` to
change the type of the EC2 instance used.

Run `make` to build the AMI:

```bash
$ cd docs/howto/assets/eks
$ make
```

You can also build the AMI with a different version of Krustlet from a forked repo. For example:

```bash
$ cd docs/howto/assets/eks
$ KRUSTLET_VERSION=$(git rev-parse --short HEAD) KRUSTLET_SRC=https://github.com/jingweno/krustlet/archive/$(git rev-parse --short HEAD).tar.gz make krustlet
```

This command will take a while to build Krustlet from source on the EC2 instance. In the future, a
prebuilt binary for Amazon Linux 2 might be available that would speed up the AMI creation process.

If everything works correctly, you should see the command complete with output similar to:

```bash
...
==> Builds finished. The artifacts of successful builds are:
--> amazon-ebs: AMIs were created:
us-west-2: ami-07adf9ce893885a3d

--> amazon-ebs:
```

Make note of the AMI identifier (in the example output above it would be `ami-07adf9ce893885a3d`) as
it will be used to create the EKS cluster.

## Creating the EKS cluster

We will be using [eksctl](https://eksctl.io/) to deploy the EKS cluster.

Create a file named `cluster.yaml` with the following contents, replacing the `region` and `ami`
fields with your values:

```yaml
apiVersion: eksctl.io/v1alpha5
kind: ClusterConfig

metadata:
  name: krustlet-demo
  region: <YOUR_AWS_REGION_HERE>
  version: "1.15"

nodeGroups:
  - name: krustlet
    ami: <YOUR_AMI_HERE>
    instanceType: t3.small
    minSize: 1
    maxSize: 3
    desiredCapacity: 2
    ssh:
      allow: true
    overrideBootstrapCommand: /etc/eks/bootstrap.sh --krustlet-node-labels "alpha.eksctl.io/cluster-name=krustlet-demo,alpha.eksctl.io/nodegroup-name=krustlet"
```

This will create a EKS cluster named `krustlet-demo` with a single unmanaged node group named
`krustlet` with two `t3.small` nodes.

Be aware that the `overrideBootstrapCommand` setting is required to properly boot the nodes. Without
it, the Krustlet service will not be started and the nodes will not automatically join the cluster.

Use `eksctl` to create the cluster:

```bash
$ eksctl create cluster -f cluster.yaml
```

This command will take a long time to run as it provisions the EKS cluster and nodes.

Eventually, the command will be stuck on the following output:

```text
...
[ℹ]  waiting for at least 1 node(s) to become ready in "krustlet"
```

With another shell, ensure the nodes have joined the cluster:

```bash
$ kubectl get nodes
NAME                                          STATUS   ROLES   AGE   VERSION
ip-192-168-24-34.us-west-2.compute.internal   Ready    agent   23s   v1.17.0
ip-192-168-44-27.us-west-2.compute.internal   Ready    agent   17s   v1.17.0
```

You should see two nodes with different names in the output.

## Running a WebAssembly application

Let's deploy a demo WebAssembly application to the cluster:

```bash
$ kubectl apply -f demos/wasi/hello-world-rust/k8s.yaml
```

Check that the pod ran to completion:

```bash
$ kubectl get pod hello-world-wasi-rust
NAME                    READY   STATUS       RESTARTS   AGE
hello-world-wasi-rust   0/1     ExitCode:0   0          7s
```

This output shows the pod completed with an exit code of 0.

Take a look at the log to see the output of the application:

```bash
$ kubectl logs hello-world-wasi-rust
hello from stdout!
hello from stderr!
POD_NAME=hello-world-wasi-rust
FOO=bar
CONFIG_MAP_VAL=cool stuff
Args are: []
```

Congratulations!  You've run a WebAssembly program on an EKS cluster!

## Deleting the cluster

Use `eksctl` to delete the cluster and the nodes:

```bash
$ eksctl delete cluster --name krustlet-demo
```

## Deleting the Krustlet AMI

Determine the snapshot identifier of the AMI, where `$AMI_ID` is the identifier of your Krustlet
AMI:

```bash
$ aws ec2 describe-images --image-ids $AMI_ID | grep SnapshotId
```

Use `aws` to deregister the AMI, where `$AMI_ID` is the identifier of your Krustlet AMI:

```bash
$ aws ec2 deregister-image --image-id $AMI_ID
```

Next, delete the snapshot, where `$SNAPSHOT_ID` is the previously determined snapshot identifier:

```bash
$ aws ec2 delete-snapshot --snapshot-id $SNAPSHOT_ID
```
