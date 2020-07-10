use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{ConfigMap, Node, Pod, Secret, Taint};
use kube::{
    api::{Api, DeleteParams, ListParams, LogParams, PostParams, WatchEvent},
    runtime::Informer,
};
use serde_json::json;

mod pod_builder;
use pod_builder::{wasmerciser_pod, WasmerciserContainerSpec, WasmerciserVolumeSpec};

#[tokio::test]
async fn test_wascc_provider() -> Result<(), Box<dyn std::error::Error>> {
    let client = kube::Client::try_default().await?;

    let nodes: Api<Node> = Api::all(client);

    let node = nodes.get("krustlet-wascc").await?;

    verify_wascc_node(node).await;

    let client: kube::Client = nodes.into();

    let _cleaner = WasccTestResourceCleaner {};

    let pods: Api<Pod> = Api::namespaced(client.clone(), "default");

    create_wascc_pod(client.clone(), &pods).await?;

    // Send a request to the pod to trigger some logging
    reqwest::get("http://127.0.0.1:30000")
        .await
        .expect("unable to perform request to test pod");

    let logs = pods
        .logs("greet-wascc", &LogParams::default())
        .await
        .expect("unable to get logs");
    assert!(logs.contains("warn something"));
    assert!(logs.contains("info something"));
    assert!(logs.contains("raw msg I'm a Body!"));
    assert!(logs.contains("error body"));

    Ok(())
}

async fn verify_wascc_node(node: Node) -> () {
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

    let taints = node
        .spec
        .expect("node had no spec")
        .taints
        .expect("node had no taints");
    let taint = taints
        .iter()
        .find(|t| t.key == "krustlet/arch")
        .expect("did not find krustlet/arch taint");
    // There is no "operator" field in the type for the crate for some reason,
    // so we can't compare it here
    assert_eq!(
        taint,
        &Taint {
            effect: "NoExecute".to_owned(),
            key: "krustlet/arch".to_owned(),
            value: Some("wasm32-wascc".to_owned()),
            ..Default::default()
        }
    );
}

async fn wait_for_pod_ready(client: kube::Client, pod_name: &str) -> anyhow::Result<()> {
    let api = Api::namespaced(client, "default");
    let inf: Informer<Pod> = Informer::new(api).params(
        ListParams::default()
            .fields(&format!("metadata.name={}", pod_name))
            .timeout(30),
    );

    let mut watcher = inf.poll().await?.boxed();
    let mut went_ready = false;
    while let Some(event) = watcher.try_next().await? {
        match event {
            WatchEvent::Modified(o) => {
                let phase = o.status.unwrap().phase.unwrap();
                if phase == "Running" {
                    went_ready = true;
                    break;
                }
            }
            WatchEvent::Error(e) => {
                panic!("WatchEvent error: {:?}", e);
            }
            _ => {}
        }
    }

    assert!(went_ready, "pod never went ready");

    Ok(())
}

async fn create_wascc_pod(client: kube::Client, pods: &Api<Pod>) -> anyhow::Result<()> {
    let p = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": "greet-wascc"
        },
        "spec": {
            "containers": [
                {
                    "name": "greet-wascc",
                    "image": "webassembly.azurecr.io/greet-wascc:v0.4",
                    "ports": [
                        {
                            "containerPort": 8080,
                            "hostPort": 30000
                        }
                    ],
                },
            ],
            "tolerations": [
                {
                    "effect": "NoExecute",
                    "key": "krustlet/arch",
                    "operator": "Equal",
                    "value": "wasm32-wascc"
                },
            ]
        }
    }))?;

    let pod = pods.create(&PostParams::default(), &p).await?;

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    wait_for_pod_ready(client, "greet-wascc").await?;

    Ok(())
}

struct WasccTestResourceCleaner {}

impl Drop for WasccTestResourceCleaner {
    fn drop(&mut self) {
        let t = std::thread::spawn(move || {
            let mut rt =
                tokio::runtime::Runtime::new().expect("Failed to reate Tokio runtime for cleanup");
            rt.block_on(clean_up_wascc_test_resources());
        });

        t.join().expect("Failed to clean up wasCC test resources");
    }
}

async fn clean_up_wascc_test_resources() -> () {
    let client = kube::Client::try_default()
        .await
        .expect("Failed to create client");

    let pods: Api<Pod> = Api::namespaced(client.clone(), "default");
    pods.delete("greet-wascc", &DeleteParams::default())
        .await
        .expect("Failed to delete pod");
}

async fn verify_wasi_node(node: Node) -> () {
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

    let taints = node
        .spec
        .expect("node had no spec")
        .taints
        .expect("node had no taints");
    let taint = taints
        .iter()
        .find(|t| t.key == "krustlet/arch")
        .expect("did not find krustlet/arch taint");
    // There is no "operator" field in the type for the crate for some reason,
    // so we can't compare it here
    assert_eq!(
        taint,
        &Taint {
            effect: "NoExecute".to_owned(),
            key: "krustlet/arch".to_owned(),
            value: Some("wasm32-wasi".to_owned()),
            ..Default::default()
        }
    );
}

const SIMPLE_WASI_POD: &str = "hello-wasi";
const VERBOSE_WASI_POD: &str = "hello-world-verbose";
const FAILY_POD: &str = "faily-pod";
const INITY_WASI_POD: &str = "hello-wasi-with-inits";

async fn create_wasi_pod(client: kube::Client, pods: &Api<Pod>) -> anyhow::Result<()> {
    let pod_name = SIMPLE_WASI_POD;
    // Create a temp directory to use for the host path
    let tempdir = tempfile::tempdir()?;
    let p = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": pod_name
        },
        "spec": {
            "containers": [
                {
                    "name": pod_name,
                    "image": "webassembly.azurecr.io/hello-wasm:v1",
                    "volumeMounts": [
                        {
                            "mountPath": "/foo",
                            "name": "secret-test"
                        },
                        {
                            "mountPath": "/bar",
                            "name": "configmap-test"
                        },
                        {
                            "mountPath": "/baz",
                            "name": "hostpath-test"
                        }
                    ]
                },
            ],
            "tolerations": [
                {
                    "effect": "NoExecute",
                    "key": "krustlet/arch",
                    "operator": "Equal",
                    "value": "wasm32-wasi"
                },
            ],
            "volumes": [
                {
                    "name": "secret-test",
                    "secret": {
                        "secretName": "hello-wasi-secret"
                    }
                },
                {
                    "name": "configmap-test",
                    "configMap": {
                        "name": "hello-wasi-configmap"
                    }
                },
                {
                    "name": "hostpath-test",
                    "hostPath": {
                        "path": tempdir.path()
                    }
                }
            ]
        }
    }))?;

    // TODO: Create a testing module to write to the path to actually check that writing and reading
    // from a host path volume works

    let pod = pods.create(&PostParams::default(), &p).await?;

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    wait_for_pod_complete(client, pod_name).await
}

async fn create_fancy_schmancy_wasi_pod(
    client: kube::Client,
    pods: &Api<Pod>,
) -> anyhow::Result<()> {
    let pod_name = VERBOSE_WASI_POD;
    let p = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": pod_name
        },
        "spec": {
            "containers": [
                {
                    "name": pod_name,
                    "image": "webassembly.azurecr.io/hello-world-wasi-rust:v0.1.0",
                    "args": [ "arg1", "arg2" ],
                },
            ],
            "tolerations": [
                {
                    "effect": "NoExecute",
                    "key": "krustlet/arch",
                    "operator": "Equal",
                    "value": "wasm32-wasi"
                },
            ],
        }
    }))?;

    let pod = pods.create(&PostParams::default(), &p).await?;

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    wait_for_pod_complete(client, pod_name).await
}

async fn create_faily_pod(client: kube::Client, pods: &Api<Pod>) -> anyhow::Result<()> {
    let pod_name = FAILY_POD;

    let containers = vec![WasmerciserContainerSpec {
        name: pod_name,
        args: &["assert_exists(file:/nope.nope.nope.txt)"],
    }];

    wasmercise_wasi(pod_name, client, pods, vec![], containers, vec![]).await
}

async fn wasmercise_wasi(
    pod_name: &str,
    client: kube::Client,
    pods: &Api<Pod>,
    inits: Vec<WasmerciserContainerSpec>,
    containers: Vec<WasmerciserContainerSpec>,
    test_volumes: Vec<WasmerciserVolumeSpec>,
) -> anyhow::Result<()> {
    let p = wasmerciser_pod(pod_name, inits, containers, test_volumes, "wasm32-wasi")?;

    let pod = pods.create(&PostParams::default(), &p.pod).await?;

    assert_eq!(pod.status.unwrap().phase.unwrap(), "Pending");

    wait_for_pod_complete(client, pod_name).await
}

async fn create_pod_with_init_containers(
    client: kube::Client,
    pods: &Api<Pod>,
) -> anyhow::Result<()> {
    let pod_name = INITY_WASI_POD;

    let inits = vec![
        WasmerciserContainerSpec {
            name: "init-1",
            args: &["write(lit:slats)to(file:/hp/floofycat.txt)"],
        },
        WasmerciserContainerSpec {
            name: "init-2",
            args: &["write(lit:kiki)to(file:/hp/neatcat.txt)"],
        },
    ];

    let containers = vec![WasmerciserContainerSpec {
        name: pod_name,
        args: &[
            "assert_exists(file:/hp/floofycat.txt)",
            "assert_exists(file:/hp/neatcat.txt)",
            "read(file:/hp/floofycat.txt)to(var:fcat)",
            "assert_value(var:fcat)is(lit:slats)",
            "write(var:fcat)to(stm:stdout)",
        ],
    }];

    let volumes = vec![WasmerciserVolumeSpec {
        volume_name: "hostpath-test",
        mount_path: "/hp",
    }];

    wasmercise_wasi(pod_name, client, pods, inits, containers, volumes).await
}

async fn wait_for_pod_complete(client: kube::Client, pod_name: &str) -> anyhow::Result<()> {
    let api = Api::namespaced(client.clone(), "default");
    let inf: Informer<Pod> = Informer::new(api).params(
        ListParams::default()
            .fields(&format!("metadata.name={}", pod_name))
            .timeout(30),
    );

    let mut watcher = inf.poll().await?.boxed();
    let mut went_ready = false;
    while let Some(event) = watcher.try_next().await? {
        match event {
            WatchEvent::Modified(o) => {
                let phase = o.status.unwrap().phase.unwrap();
                if phase == "Running" {
                    went_ready = true;
                }
                if phase == "Succeeded" && !went_ready {
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

    assert!(went_ready, format!("pod {} never went ready", pod_name));

    Ok(())
}

async fn set_up_wasi_test_environment(client: kube::Client) -> anyhow::Result<()> {
    let secrets: Api<Secret> = Api::namespaced(client.clone(), "default");
    secrets
        .create(
            &PostParams::default(),
            &serde_json::from_value(json!({
                "apiVersion": "v1",
                "kind": "Secret",
                "metadata": {
                    "name": "hello-wasi-secret"
                },
                "stringData": {
                    "myval": "a cool secret"
                }
            }))?,
        )
        .await?;

    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), "default");
    config_maps
        .create(
            &PostParams::default(),
            &serde_json::from_value(json!({
                "apiVersion": "v1",
                "kind": "ConfigMap",
                "metadata": {
                    "name": "hello-wasi-configmap"
                },
                "data": {
                    "myval": "a cool configmap"
                }
            }))?,
        )
        .await?;

    Ok(())
}

async fn clean_up_wasi_test_resources() -> () {
    let client = kube::Client::try_default()
        .await
        .expect("Failed to create client");

    let secrets: Api<Secret> = Api::namespaced(client.clone(), "default");
    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), "default");
    let pods: Api<Pod> = Api::namespaced(client.clone(), "default");

    let cleanup_errors: Vec<_> = vec![
        secrets
            .delete("hello-wasi-secret", &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("secret hello-wasi-secret ({})", e)),
        config_maps
            .delete("hello-wasi-configmap", &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("configmap hello-wasi-configmap ({})", e)),
        pods.delete(SIMPLE_WASI_POD, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("pod {} ({})", SIMPLE_WASI_POD, e)),
        pods.delete(VERBOSE_WASI_POD, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("pod {} ({})", VERBOSE_WASI_POD, e)),
        pods.delete(FAILY_POD, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("pod {} ({})", FAILY_POD, e)),
        pods.delete(INITY_WASI_POD, &DeleteParams::default())
            .await
            .err()
            .map(|e| format!("pod {} ({})", INITY_WASI_POD, e)),
    ]
    .iter()
    .filter(|e| e.is_some())
    .map(|e| e.as_ref().unwrap().to_string())
    .collect();

    if !cleanup_errors.is_empty() {
        let cleanup_failure_text = format!(
            "Error(s) cleaning up resources: {}",
            cleanup_errors.join(", ")
        );
        assert!(false, cleanup_failure_text);
    }
}

struct WasiTestResourceCleaner {}

impl Drop for WasiTestResourceCleaner {
    fn drop(&mut self) {
        let t = std::thread::spawn(move || {
            let mut rt =
                tokio::runtime::Runtime::new().expect("Failed to reate Tokio runtime for cleanup");
            rt.block_on(clean_up_wasi_test_resources());
        });

        t.join().expect("Failed to clean up WASI test resources");
    }
}

#[tokio::test]
async fn test_wasi_provider() -> anyhow::Result<()> {
    let client = kube::Client::try_default().await?;

    let nodes: Api<Node> = Api::all(client);

    let node = nodes.get("krustlet-wasi").await?;

    verify_wasi_node(node).await;

    let client: kube::Client = nodes.into();

    set_up_wasi_test_environment(client.clone()).await?;

    let _cleaner = WasiTestResourceCleaner {};

    let pods: Api<Pod> = Api::namespaced(client.clone(), "default");

    create_wasi_pod(client.clone(), &pods).await?;

    assert_pod_log_equals(&pods, SIMPLE_WASI_POD, "Hello, world!\n").await?;

    assert_pod_exited_successfully(&pods, SIMPLE_WASI_POD).await?;

    // TODO: Create a module that actually reads from a directory and outputs to logs
    assert_container_file_contains(
        "secret-test/myval",
        "a cool secret",
        "unable to open secret file",
    )
    .await?;
    assert_container_file_contains(
        "configmap-test/myval",
        "a cool configmap",
        "unable to open configmap file",
    )
    .await?;

    create_fancy_schmancy_wasi_pod(client.clone(), &pods).await?;

    assert_pod_log_contains(&pods, VERBOSE_WASI_POD, r#"Args are: ["arg1", "arg2"]"#).await?;

    create_faily_pod(client.clone(), &pods).await?;

    assert_pod_exited_with_failure(&pods, FAILY_POD).await?;
    assert_pod_log_contains(
        &pods,
        FAILY_POD,
        r#"ERR: Failed with File /nope.nope.nope.txt was expected to exist but did not"#,
    )
    .await?;

    create_pod_with_init_containers(client.clone(), &pods).await?;

    assert_pod_log_contains(&pods, INITY_WASI_POD, r#"slats"#).await?;

    Ok(())
}

async fn assert_pod_log_equals(
    pods: &Api<Pod>,
    pod_name: &str,
    expected_log: &str,
) -> anyhow::Result<()> {
    let mut logs = pods.log_stream(pod_name, &LogParams::default()).await?;

    while let Some(chunk) = logs.try_next().await? {
        assert_eq!(expected_log, String::from_utf8_lossy(&chunk));
    }

    Ok(())
}

async fn assert_pod_log_contains(
    pods: &Api<Pod>,
    pod_name: &str,
    expected_log: &str,
) -> anyhow::Result<()> {
    let mut logs = pods.log_stream(pod_name, &LogParams::default()).await?;
    let mut log_chunks: Vec<String> = Vec::default();

    while let Some(chunk) = logs.try_next().await? {
        let chunk_text = String::from_utf8_lossy(&chunk);
        if chunk_text.contains(expected_log) {
            return Ok(()); // can early exit if the expected value is entirely within a chunk
        } else {
            log_chunks.push(chunk_text.to_string());
        }
    }

    let actual_log_text = log_chunks.join("");
    assert!(
        actual_log_text.contains(expected_log),
        format!(
            "Expected log containing {} but got {}",
            expected_log, actual_log_text
        )
    );
    Ok(())
}

async fn assert_pod_exited_successfully(pods: &Api<Pod>, pod_name: &str) -> anyhow::Result<()> {
    let pod = pods.get(pod_name).await?;

    let state = (|| {
        pod.status?.container_statuses?[0]
            .state
            .as_ref()?
            .terminated
            .clone()
    })()
    .expect("Could not fetch terminated states");
    assert_eq!(state.exit_code, 0);

    Ok(())
}

async fn assert_pod_exited_with_failure(pods: &Api<Pod>, pod_name: &str) -> anyhow::Result<()> {
    let pod = pods.get(pod_name).await?;

    let state = (|| {
        pod.status?.container_statuses?[0]
            .state
            .as_ref()?
            .terminated
            .clone()
    })()
    .expect("Could not fetch terminated states");
    assert_eq!(state.exit_code, 1);

    Ok(())
}

async fn assert_container_file_contains(
    container_file_path: &str,
    expected_content: &str,
    file_error: &str,
) -> anyhow::Result<()> {
    let file_path_base = dirs::home_dir()
        .expect("home dir does not exist")
        .join(".krustlet/volumes/hello-wasi-default");
    let container_file_bytes = tokio::fs::read(file_path_base.join(container_file_path))
        .await
        .expect(file_error);
    assert_eq!(
        expected_content.to_owned().into_bytes(),
        container_file_bytes
    );
    Ok(())
}
