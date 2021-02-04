# Writing your first app, part 3

This tutorial begins where [Tutorial 2](tutorial02.md) left off. We’ll walk
through the process for installing your first application written in WebAssembly
into your Kubernetes cluster, then test our application using `kubectl`.

## Scheduling pods on the Krustlet

In Kubernetes, Pods are the smallest deployable units of compute that can be
created and managed in Kubernetes. In other words, your application runs inside
a Pod, and we can inspect the status of the application by inspecting the Pod.

Krustlet listens for pods requesting a node with the `wasm32-wasi` architecture.
To schedule a Pod that Krustlet understands, we need to provide Kubernetes with
a YAML file describing our Pod.

Create a new file and call it `krustlet-tutorial.yaml`:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: krustlet-tutorial
spec:
  containers:
    - name: krustlet-tutorial
      image: mycontainerregistry007.azurecr.io/krustlet-tutorial:v1.0.0
  imagePullSecrets:
    - name: <acr-secret>
  tolerations:
    - key: "kubernetes.io/arch"
      operator: "Equal"
      value: "wasm32-wasi"
      effect: "NoExecute"
    - key: "kubernetes.io/arch"
      operator: "Equal"
      value: "wasm32-wasi"
      effect: "NoSchedule"
```

Let's break this file down:

- `apiVersion`: which version of the Kubernetes API are we targeting?
- `kind`: what type of workload are we deploying?
- `metadata.name`: what is the name of our workload?
- `spec.containers[0].name`: what should I name this module?
- `spec.containers[0].image`: where can I find the module?
- `spec.imagePullSecrets[0].name`: which name has the  image pull secret?
- `spec.tolerations`: what kind of node am I allowed to run on?

To deploy this workload to Kubernetes, we use `kubectl`.

```console
$ kubectl create -f krustlet-tutorial.yaml
```

Now that the workload has been scheduled, Krustlet should start spewing out some
logs in its terminal window, reporting updates on the workload that was
scheduled.

We can check the status of our pod:

```console
$ kubectl get pods
NAME                READY   STATUS    RESTARTS   AGE
krustlet-tutorial   1/1     Running   0          18s
```

We can also inspect the logs, too:

```console
$ kubectl logs krustlet-tutorial
Hello, World!
Hello, World!
Hello, World!
Hello, World!
Hello, World!
```

## Cleanup

Once you're finished with this tutorial, you can destroy the cluster and the
registry.

Destroying the cluster can be accomplished with:

```console
$ kind delete cluster
```

And destroying the registry can be accomplished by removing the resource group.

```console
$ az group delete --name myResourceGroup
```

## Conclusion

This concludes the basic tutorial. Congratulations!

If you are familiar with Krustlet and are interested in more in-depth topics,
check out the [Topic Guides](../topics/README.md).

You might also be scratching your head on what to [read next](readnext.md).
