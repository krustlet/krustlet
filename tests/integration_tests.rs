use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{Node, Pod};
use kube::{
    api::{Api, DeleteParams, ListParams, PostParams, Resource, WatchEvent},
    client::APIClient,
    config,
    runtime::Informer,
};
use serde_json::json;

#[tokio::test]
async fn test_wascc_provider() {
    // Read the environment. Note that this tries a KubeConfig file first, then
    // falls back on an in-cluster configuration.
    let kubeconfig = config::load_kube_config()
        .await
        .or_else(|_| config::incluster_config())
        .expect("kubeconfig failed to load");

    let client = APIClient::new(kubeconfig);

    let nodes: Api<Node> = Api::all(client.clone());

    let node = nodes
        .get("krustlet-wascc")
        .await
        .expect("failed to find node with name 'krustlet-wascc'");

    let node_status = node.status.expect("node reported no status");
    assert_eq!(
        node_status
            .node_info
            .expect("node reported no information")
            .architecture,
        "wasm-wasi",
        "expected node to support the wasm-wasi architecture"
    );

    let node_meta = node.metadata.expect("node reported no metadata");
    assert_eq!(
        node_meta
            .labels
            .expect("node had no labels")
            .get("kubernetes.io/arch")
            .expect("node did not have kubernetes.io/arch label"),
        "wasm32-wascc"
    );

    let pods: Api<Pod> = Api::namespaced(client.clone(), "default");
    let p = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": "hello-wascc",
            "annotations": {
                "deislabs.io/wascc-action-key": "MB4OLDIC3TCZ4Q4TGGOVAZC43VXFE2JQVRAXQMQFXUCREOOFEKOKZTY2"
            }
        },
        "spec": {
            "containers": [
                {
                    "name": "hello-wascc",
                    "image": "echo.wasm",
                },
            ],
            "nodeSelector": {
                "beta.kubernetes.io/arch": "wasm32-wascc",
            },
        }
    })).expect("failed to deserialize pod spec from JSON");

    let pod = pods
        .create(&PostParams::default(), &p)
        .await
        .expect("could not create pod");

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    let inf: Informer<Pod> = Informer::new(
        client,
        ListParams::default()
            .fields("metadata.name=hello-wascc")
            .timeout(10),
        Resource::namespaced::<Pod>("default"),
    );

    let mut watcher = inf
        .poll()
        .await
        .expect("failed to poll for pod events")
        .boxed();

    while let Some(event) = watcher
        .try_next()
        .await
        .expect("failed to poll for a pod event")
    {
        match event {
            WatchEvent::Modified(o) => {
                let phase = o.status.unwrap().phase.unwrap();
                if phase == "Running" {
                    break;
                }
            }
            WatchEvent::Error(e) => {
                panic!("WatchEvent error: {:?}", e);
            }
            _ => {}
        }
    }

    // TODO: check pod logs

    // cleanup
    pods.delete("hello-wascc", &DeleteParams::default())
        .await
        .expect("could not delete pod");
}

#[tokio::test]
async fn test_wasi_provider() {
    // Read the environment. Note that this tries a KubeConfig file first, then
    // falls back on an in-cluster configuration.
    let kubeconfig = config::load_kube_config()
        .await
        .or_else(|_| config::incluster_config())
        .expect("kubeconfig failed to load");

    let client = APIClient::new(kubeconfig);

    let nodes: Api<Node> = Api::all(client.clone());

    let node = nodes
        .get("krustlet-wasi")
        .await
        .expect("failed to find node with name 'krustlet-wasi'");

    let node_status = node.status.expect("node reported no status");
    assert_eq!(
        node_status
            .node_info
            .expect("node reported no information")
            .architecture,
        "wasm-wasi",
        "expected node to support the wasm-wasi architecture"
    );

    let node_meta = node.metadata.expect("node reported no metadata");
    assert_eq!(
        node_meta
            .labels
            .expect("node had no labels")
            .get("kubernetes.io/arch")
            .expect("node did not have kubernetes.io/arch label"),
        "wasm32-wasi"
    );

    let pods: Api<Pod> = Api::namespaced(client.clone(), "default");
    let p = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": "hello-wasi"
        },
        "spec": {
            "containers": [
                {
                    "name": "hello-wasi",
                    "image": "webassembly.azurecr.io/hello-wasm:v1",
                },
            ],
            "nodeSelector": {
                "beta.kubernetes.io/arch": "wasm32-wasi",
            },
        }
    }))
    .expect("failed to deserialize pod spec from JSON");

    let pod = pods
        .create(&PostParams::default(), &p)
        .await
        .expect("could not create pod");

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    let inf: Informer<Pod> = Informer::new(
        client,
        ListParams::default()
            .fields("metadata.name=hello-wasi")
            .timeout(10),
        Resource::namespaced::<Pod>("default"),
    );

    let mut watcher = inf
        .poll()
        .await
        .expect("failed to poll for pod events")
        .boxed();

    while let Some(event) = watcher
        .try_next()
        .await
        .expect("failed to poll for a pod event")
    {
        match event {
            WatchEvent::Modified(o) => {
                let phase = o.status.unwrap().phase.unwrap();
                if phase == "Running" {
                    break;
                }
            }
            WatchEvent::Error(e) => {
                panic!("WatchEvent error: {:?}", e);
            }
            _ => {}
        }
    }

    // TODO: check pod logs

    // cleanup
    pods.delete("hello-wasi", &DeleteParams::default())
        .await
        .expect("could not delete pod");
}
