# Running Kubernetes on Google Kubernetes Engine (GKE)

[Google Kubernetes Engine (GKE)](https://cloud.google.com/kubernetes-engine) is a secured and managed Kubernetes service.

If you haven't used Google Cloud Platform, you'll need a Google (e.g. Gmail) account. As a new customer, you may benefit from $300 free credit. Google Cloud Platform includes always free products. See [Google Cloud Platform Free Tier](https://cloud.google.com/free)


## Prerequisites

You should be able to run [Google Cloud SDK ](https://cloud.google.com/sdk) command-line tool `gcloud`. This is used to provision resources in Google Cloud Platform including Kubernetes clusters.

Either install [Google Cloud SDK](https://cloud.google.com/sdk/install) or open a [Cloud Shell](https://console.cloud.google.com/home/dashboard?cloudshell=true).

Google Cloud SDK is available for Linux, Windows and Mac OS. The instructions that follow document using the command-line on Linux. There may be subtle changes for Windows and Mac OS.

Google Cloud Platform provides a browser-based [Console](https://console.cloud.google.com). This is generally functionally equivalent to the command-line tool. The instructions that follow document using the command-line tool but you may perform these steps using the Console too.

You will also need Kubernetes command-line tool `kubectl`. `kubectl` is used by all Kubernetes distributions. So, if you've created Kubernetes clusters locally or on other cloud platforms, you may already have this tool installed. See [Install and Set Up kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl/) for instructions.

## Configure Google Cloud CLI

After installing Google Cloud SDK, you will need to initialize the tool. This also authenticates your account using a Google identity (e.g. Gmail). Do this by typing `gcloud init`. If for any reason, you have already run `gcloud init`, you may reauthenticate using `gcloud auth login` or check authentication with `gcloud auth list`.

## Create GKE cluster

Google Cloud Platform resources are aggregated by projects. Projects are assigned to Billing Accounts. GKE uses Compute Engine VMs as nodes and Compute Engine VMs require that assign a Billing Account to our project so that we may pay for the VMs.

```shell
$ PROJECT=[YOUR-PROJECT] # Perhaps $(whoami)-$(date +%y%m%d)-krustlet
$ BILLING=[YOUR-BILLING] # You may list these using `gcloud beta billing accounts list`
$ # Create Project and assing Billing Account
$ gcloud projects create ${PROJECT}
$ gcloud alpha billing projects link ${PROJECT} --billing-account=${BILLING}
$ # Enable Kubernetes Engine & Compute Engine
$ gcloud services enable container.googleapis.com --project=${PROJECT}
$ gcloud services enable compute.googleapis.com --project=${PROJECT}
$ REGION="us-west1" # Use a region close to you `gcloud compute regions list --project=${PROJECT}`
$ CLUSTER="cluster"
$ # Create GKE cluster with 3 nodes (one per zone in the region)
$ gcloud beta container clusters create ${CLUSTER} \
--project=${PROJECT} \
--region=${REGION} \
--no-enable-basic-auth \
--release-channel "rapid" \
--machine-type "n1-standard-1" \
--image-type "COS_CONTAINERD" \
--preemptible \
--num-nodes="1"
```

> **NOTE** This creates a cluster with nodes distributed across multiple zones in a region. This
increases the cluster's availability. If you'd prefer a less available (and cheaper) single zone
cluster, you may use the following commands instead:

```shell
$ ZONE="${REGION}-a" # Or "-b" or "-c"
$ gcloud beta container clusters create ${CLUSTER} \
--project=${PROJECT} \
--zone=${ZONE} \
--no-enable-basic-auth \
--release-channel "rapid" \
--machine-type "n1-standard-1" \
--image-type "COS_CONTAINERD" \
--preemptible \
--num-nodes="1"
```

After a minute, you should see the cluster created:

```shell
NAME     LOCATION  MASTER_VERSION  MASTER_IP       MACHINE_TYPE   NODE_VERSION   NUM_NODES  STATUS
cluster  us-west1  1.17.4-gke.10   xx.xx.xx.xx     n1-standard-1  1.17.4-gke.10  3          RUNNING
```

> **NOTE** You may also use Cloud Console to interact with the cluster:
https://console.cloud.google.com/kubernetes/list?project=${PROJECT}

> **NOTE** `gcloud clusters create` also configures `kubectl` to be able to access the cluster.

You may confirm access to the cluster by typing:

```shell
$ kubectl get nodes
NAME                                     STATUS   ROLES    AGE   VERSION
gke-cluster-default-pool-1a3a5b85-scds   Ready    <none>   10m   v1.17.4-gke.10
gke-cluster-default-pool-3885c0e3-6zw2   Ready    <none>   10m   v1.17.4-gke.10
gke-cluster-default-pool-6d70a85d-19r8   Ready    <none>   10m   v1.17.4-gke.10
```

You may confirm the Kubernetes configuration either by:

```shell
$ more ${HOME}/.kube/config
apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: LS0tLS1C...
    server: https://xx.xx.xx.xx
  name: gke_${PROJECT}_${REGION}_${CLUSTER}
contexts:
- context:
    cluster: gke_${PROJECT}_${REGION}_${CLUSTER}
    user: gke_${PROJECT}_${REGION}_${CLUSTER}
  name: gke_${PROJECT}_${REGION}_${CLUSTER}
current-context: gke_${PROJECT}_${REGION}_${CLUSTER}
kind: Config
preferences: {}
users:
- name: gke_${PROJECT}_${REGION}_${CLUSTER}
  user:
    auth-provider:
      config:
        cmd-args: config config-helper --format=json
        cmd-path: /snap/google-cloud-sdk/130/bin/gcloud
        expiry-key: '{.credential.token_expiry}'
        token-key: '{.credential.access_token}'
      name: gcp
```

Or:

```shell
$ kubectl config current-context
gke_${PROJECT}_${REGION}_${CLUSTER}
$ kubectl config get-contexts
CURRENT   NAME                                  CLUSTER                               AUTHINFO
*         gke_${PROJECT}_${REGION}_${CLUSTER}   gke_${PROJECT}_${REGION}_${CLUSTER}   gke_${PROJECT}_${REGION}_${CLUSTER}
```

## Delete the Cluster

When you are finished with the cluster, you may delete it with:

```shell
$ gcloud beta container clusters delete ${CLUSTER} --project=${PROJECT} --region=${REGION} --quiet
```

If you wish to delete everything in the project, you may delete hte project (including all its
resources) with:

```shell
$ gcloud projects delete ${PROJECT} --quiet
```

> **NOTE** Both commands are irrevocable.
