# Krustlet: Kubernetes Kubelet in Rust for running WASM

**This project is highly experimental.** It is just a proof of concept, and you should not use it in production.

Krustlet acts as a Kubelet by listening on the event stream for new pod requests that match a particular set of node selectors.

The default implementation of Krustlet listens for the architecture `wasm32-wasi` and schedules those workloads to run in a `wasmtime`-based runtime instead of a container runtime.

## Building

We recommend using [just](https://github.com/casey/just) to build. But you can just use `cargo` if you want:

```console
$ just build
$ cargo build
```

Building a Docker image is easy, too:

```console
$ just dockerize
```

That will take a LOOONG time the first build, but the layer cache will make it much faster from then on.

## Running

Again, we recommend `just`, but you can use `cargo`:

```console
$ just run
$ cargo run
```

Note that if you are not running the binary in your cluster (e.g. if you are instead running it locally), then the `log` and `exec` calls will result in errors.

## Scheduling Pods on the Krustlet

The krustlet listens for wasm32-wasi architecture:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: greet
spec:
  containers:
    - image: technosophos/greet
      imagePullPolicy: Always
      name: greet
  nodeSelector:
    kubernetes.io/role: agent
    beta.kubernetes.io/os: linux
    beta.kubernetes.io/arch: wasm32-wasi
```

Note that the `nodeSelector` is the important part above, though `image` is expected to point to a WASM module as well.

To load the above into Kubernetes, use `kubectl apply -f greet.yaml`. You should see the pod go into the `Running` state very quickly. If the WASM is not daemonized, it should go to the `Succeeded` phase soon thereafter.

## Creating your own Kubelets with Krustlet

If you want to create your own Kubelet based on Krustlet, all you need to do is implement a `Provider`. See the `src/main.rs` to get started.