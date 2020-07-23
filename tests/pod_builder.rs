use k8s_openapi::api::core::v1::{Container, Pod, Volume, VolumeMount};
use serde_json::json;
use std::sync::Arc;

pub struct PodLifetimeOwner {
    pub pod: Pod,
    _tempdirs: Vec<Arc<tempfile::TempDir>>, // only to keep the directories alive
}

pub struct WasmerciserContainerSpec {
    pub name: &'static str,
    pub args: &'static [&'static str],
}

pub struct WasmerciserVolumeSpec {
    pub volume_name: &'static str,
    pub mount_path: &'static str,
}

fn wasmerciser_container(
    spec: &WasmerciserContainerSpec,
    volumes: &Vec<WasmerciserVolumeSpec>,
) -> anyhow::Result<Container> {
    let volume_mounts: Vec<_> = volumes
        .iter()
        .map(|v| wasmerciser_volume_mount(v).unwrap())
        .collect();
    let container: Container = serde_json::from_value(json!({
        "name": spec.name,
        "image": "webassembly.azurecr.io/wasmerciser:v0.1.0",
        "args": spec.args,
        "volumeMounts": volume_mounts,
    }))?;
    Ok(container)
}

fn wasmerciser_volume_mount(spec: &WasmerciserVolumeSpec) -> anyhow::Result<VolumeMount> {
    let mount: VolumeMount = serde_json::from_value(json!({
        "mountPath": spec.mount_path,
        "name": spec.volume_name
    }))?;
    Ok(mount)
}

fn wasmerciser_volume(
    spec: &WasmerciserVolumeSpec,
) -> anyhow::Result<(Volume, Arc<tempfile::TempDir>)> {
    let tempdir = Arc::new(tempfile::tempdir()?);

    let volume: Volume = serde_json::from_value(json!({
        "name": spec.volume_name,
        "hostPath": {
            "path": tempdir.path()
        }
    }))?;

    Ok((volume, tempdir))
}

pub fn wasmerciser_pod(
    pod_name: &str,
    inits: Vec<WasmerciserContainerSpec>,
    containers: Vec<WasmerciserContainerSpec>,
    test_volumes: Vec<WasmerciserVolumeSpec>,
    architecture: &str,
) -> anyhow::Result<PodLifetimeOwner> {
    let init_container_specs: Vec<_> = inits
        .iter()
        .map(|spec| wasmerciser_container(spec, &test_volumes).unwrap())
        .collect();
    let app_container_specs: Vec<_> = containers
        .iter()
        .map(|spec| wasmerciser_container(spec, &test_volumes).unwrap())
        .collect();

    let volume_maps: Vec<_> = test_volumes
        .iter()
        .map(|spec| wasmerciser_volume(spec).unwrap())
        .collect();
    let (volumes, tempdirs) = unzip(&volume_maps);

    let pod = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": pod_name
        },
        "spec": {
            "initContainers": init_container_specs,
            "containers": app_container_specs,
            "tolerations": [
                {
                    "effect": "NoExecute",
                    "key": "krustlet/arch",
                    "operator": "Equal",
                    "value": architecture,
                },
            ],
            "nodeSelector": {
                "kubernetes.io/arch": architecture
            },
            "volumes": volumes,
        }
    }))?;

    Ok(PodLifetimeOwner {
        pod,
        _tempdirs: tempdirs,
    })
}

fn unzip<T, U: Clone>(source: &Vec<(T, U)>) -> (Vec<&T>, Vec<U>) {
    let ts: Vec<_> = source.iter().map(|v| &v.0).collect();
    let us: Vec<_> = source.iter().map(|v| v.1.clone()).collect();
    (ts, us)
}
