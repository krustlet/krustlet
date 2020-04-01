# Quick Start

Get started with Krustlet in three easy steps:

1. Boot a Kubernetes cluster
2. Boot a Krustlet node
3. Deploy your first application

As Krustlet is under active development, this guide will help you set up a cluster suitable for
evaluation, development, and testing purposes.

## Step 1: Boot a Kubernetes Cluster

There are many ways to boot up a Kubernetes cluster. You may choose to get up and running in cloud
environments or locally on your laptop.

If you have already created a Kubernetes cluster, proceed to the next step to install Krustlet on
your own Kubernetes cluster.

For production use:

- [Azure Kubernetes Service (AKS)](../howto/kubernetes-on-aks.md)
- [Amazon Elastic Kubernetes Service (EKS)](../howto/kubernetes-on-eks.md)

For development and evaluation purposes, it may make sense to use a VM-based Kubernetes cluster for
quick and easy setup and teardown:

- [Kubernetes-in-Docker (KinD)](../howto/kubernetes-on-kind.md)

## Step 2: Boot a Krustlet Node

Depending on whatever provider you chose in step 1, you now have a few options to boot and register
Krustlet with your Kubernetes cluster.

If you have your own Kubernetes cluster, you may want to follow the steps in the cloud-based option
guides to determine how to set up Krustlet for your own infrastructure.

For production use, you'll want to boot Krustlet on a device that can start a web server on an IP
accessible from the Kubernetes control plane.

- [Azure Kubernetes Service (AKS)](../howto/krustlet-on-aks.md)
- [Amazon Elastic Kubernetes Service (EKS)](../howto/krustlet-on-eks.md)

For testing/development environments:

- [Kubernetes-in-Docker (KinD)](../howto/krustlet-on-kind.md)

## Step 3: Deploy your First Application

Last but not least, [follow through the tutorial](tutorial01.md) to deploy your first application.


[development guide]: ../community/developers.md
