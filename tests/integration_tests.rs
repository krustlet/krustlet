use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{Node, Pod};
use kube::{
    api::{Api, DeleteParams, ListParams, LogParams, PostParams, Resource, WatchEvent},
    config,
    runtime::Informer,
};
use serde_json::json;

#[tokio::test]
async fn test_wascc_provider() -> Result<(), Box<dyn std::error::Error>> {
    // Read the environment. Note that this tries a KubeConfig file first, then
    // falls back on an in-cluster configuration.
    let kubeconfig = config::load_kube_config()
        .await
        .or_else(|_| config::incluster_config())?;

    let client = kube::Client::from(kubeconfig);

    let nodes: Api<Node> = Api::all(client.clone());

    let node = nodes.get("krustlet-wascc").await?;
    let node_status = node.status.expect("node reported no status");
    assert_eq!(
        node_status
            .node_info
            .expect("node status reported no info")
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
                    "image": "webassembly.azurecr.io/hello-wascc:v0.3",
                },
            ],
            "nodeSelector": {
                "beta.kubernetes.io/arch": "wasm32-wascc",
            },
        }
    }))?;

    let pod = pods.create(&PostParams::default(), &p).await?;

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    let inf: Informer<Pod> = Informer::new(
        client,
        ListParams::default()
            .fields("metadata.name=hello-wascc")
            .timeout(10),
        Resource::namespaced::<Pod>("default"),
    );

    let mut watcher = inf.poll().await?.boxed();

    while let Some(event) = watcher.try_next().await? {
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

    let mut logs = pods
        .log_stream("hello-wascc", &LogParams::default())
        .await?;

    while let Some(line) = logs.try_next().await? {
        assert_eq!("{\"kind\":\"Status\",\"apiVersion\":\"v1\",\"metadata\":{},\"status\":\"Failure\",\"message\":\"an error on the server (\\\"Not Implemented\\\") has prevented the request from succeeding ( pods/log hello-wascc)\",\"reason\":\"InternalError\",\"details\":{\"name\":\"hello-wascc\",\"kind\":\"pods/log\"},\"code\":501}\n", String::from_utf8_lossy(&line));
    }

    // cleanup
    pods.delete("hello-wascc", &DeleteParams::default()).await?;

    Ok(())
}

#[tokio::test]
async fn test_wasi_provider() -> Result<(), Box<dyn std::error::Error>> {
    // Read the environment. Note that this tries a KubeConfig file first, then
    // falls back on an in-cluster configuration.
    let kubeconfig = config::load_kube_config()
        .await
        .or_else(|_| config::incluster_config())?;

    let client = kube::Client::from(kubeconfig);

    let nodes: Api<Node> = Api::all(client.clone());

    let node = nodes.get("krustlet-wasi").await?;

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
    }))?;

    let pod = pods.create(&PostParams::default(), &p).await?;

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    let inf: Informer<Pod> = Informer::new(
        client,
        ListParams::default()
            .fields("metadata.name=hello-wasi")
            .timeout(10),
        Resource::namespaced::<Pod>("default"),
    );

    let mut watcher = inf.poll().await?.boxed();
    let mut found_running = false;
    while let Some(event) = watcher.try_next().await? {
        match event {
            WatchEvent::Modified(o) => {
                let phase = o.status.unwrap().phase.unwrap();
                if phase == "Running" {
                    found_running = true;
                }
                if phase == "Succeeded" && !found_running {
                    panic!("Reached completed phase before receiving Running phase")
                } else if phase == "Succeeded" {
                    break;
                }
            }
            WatchEvent::Error(e) => {
                panic!("WatchEvent error: {:?}", e);
            }
            _ => {}
        }
    }

    let mut logs = pods.log_stream("hello-wasi", &LogParams::default()).await?;

    while let Some(line) = logs.try_next().await? {
        assert_eq!("Hello, world!\n", String::from_utf8_lossy(&line));
    }

    let pod = pods.get("hello-wasi").await?;

    let state = (|| {
        pod.status?.container_statuses?[0]
            .state
            .as_ref()?
            .terminated
            .clone()
    })()
    .expect("Could not fetch terminated states");
    assert_eq!(state.exit_code, 0);

    // cleanup
    pods.delete("hello-wasi", &DeleteParams::default()).await?;

    Ok(())
}
