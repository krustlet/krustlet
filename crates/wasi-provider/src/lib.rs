mod wasi_runtime;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use kube::client::APIClient;
use kubelet::pod::{pod_status, Pod};
use kubelet::{Phase, Provider, ProviderError, Status};
use log::{debug, info};
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::stream::StreamExt;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use wasi_runtime::WasiRuntime;

const TARGET_WASM32_WASI: &str = "wasm32-wasi";

// PodStore contains a map of a unique pod key pointing to a map of container
// names to the join handle and logging for their running task
type PodStore = HashMap<String, HashMap<String, (BufReader<File>, JoinHandle<()>)>>;

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec
#[derive(Clone, Default)]
pub struct WasiProvider {
    handles: Arc<RwLock<PodStore>>,
}

#[async_trait::async_trait]
impl Provider for WasiProvider {
    async fn init(&self) -> Result<(), failure::Error> {
        Ok(())
    }

    fn arch(&self) -> String {
        TARGET_WASM32_WASI.to_string()
    }

    fn can_schedule(&self, pod: &Pod) -> bool {
        // If there is a node selector and it has arch set to wasm32-wasi, we can
        // schedule it.
        pod.spec
            .as_ref()
            .and_then(|s| s.node_selector.as_ref())
            .and_then(|i| {
                i.get("beta.kubernetes.io/arch")
                    .map(|v| v.eq(&TARGET_WASM32_WASI))
            })
            .unwrap_or(false)
    }

    async fn add(&self, pod: Pod, client: APIClient) -> Result<(), failure::Error> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.
        debug!(
            "Pod added {:?}",
            pod.metadata.as_ref().and_then(|m| m.name.as_ref())
        );
        let namespace = pod
            .metadata
            .as_ref()
            .and_then(|m| m.namespace.as_deref())
            .unwrap_or_else(|| "default");

        // TODO: Implement this for real.
        // Okay, so here is where things are REALLY unfinished. Right now, we are
        // only running the first container in a pod. And we are not using the
        // init containers at all. And they are not executed on their own threads.
        // So this is basically a toy.
        //
        // What it should do:
        // - for each volume
        //   - set up the volume map
        // - for each init container:
        //   - set up the runtime
        //   - mount any volumes (preopen)
        //   - run it to completion
        //   - bail with an error if it fails
        // - for each container and ephemeral_container
        //   - set up the runtime
        //   - mount any volumes (popen)
        //   - run it to completion
        //   - bail if it errors
        let first_container = pod.spec.as_ref().map(|s| s.containers[0].clone()).unwrap();

        let env = self.env_vars(client.clone(), &first_container, &pod).await;

        // TODO: Replace with actual image store lookup when it is merged
        let runtime = WasiRuntime::new(
            PathBuf::from("./testdata/hello-world.wasm"),
            env,
            Vec::default(),
            HashMap::default(),
        )?;

        // Get a separate file handle that can be moved onto the thread
        let (input, output) = tokio::task::spawn_blocking(move || {
            // TODO: Actual log path configuration
            let tempfile = NamedTempFile::new_in(std::env::current_dir()?)?;
            Ok::<_, failure::Error>((tempfile.reopen()?, tempfile.reopen()?))
        })
        .await
        .expect("Could not spawn worker to open temp files")?;

        let handle = tokio::task::spawn_blocking(move || {
            runtime
                .run(input)
                .expect("Could not spawn worker to run runtime")
        });
        {
            let mut handles = self.handles.write().await;
            handles.entry(key_from_pod(&pod)).or_default().insert(
                first_container.name,
                (BufReader::new(tokio::fs::File::from_std(output)), handle),
            );
        }

        info!("Pod is executing on a thread");

        pod_status(client, &pod, "Running", namespace).await;
        Ok(())
    }

    async fn modify(&self, pod: Pod, _client: APIClient) -> Result<(), failure::Error> {
        // Modify will be tricky. Not only do we need to handle legitimate modifications, but we
        // need to sift out modifications that simply alter the status. For the time being, we
        // just ignore them, which is the wrong thing to do... except that it demos better than
        // other wrong things.
        info!("Pod modified");
        info!(
            "Modified pod spec: {}",
            serde_json::to_string_pretty(&pod.status.unwrap()).unwrap()
        );
        Ok(())
    }

    async fn delete(&self, _pod: Pod, _client: APIClient) -> Result<(), failure::Error> {
        // There is currently no way to stop a long running instance, so we are
        // SOL here until there is support for it. See
        // https://github.com/bytecodealliance/wasmtime/issues/860 for more
        // information
        unimplemented!("cannot stop a running wasmtime instance")
    }

    async fn status(&self, _pod: Pod, _client: APIClient) -> Result<Status, failure::Error> {
        // TODO(taylor): Figure out the best way to check if a future is still
        // running. I get the feeling that manually calling `poll` on the future
        // is a Bad Idea™ and so I am not sure if there is another way or if we
        // should implement messaging using channels to let the main runtime
        // know it is done
        // let fut = async {
        //     let handles = self.handles.read().await;
        //     let containers = handles.get(key_from_pod(&pod));
        // };
        Ok(Status {
            phase: Phase::Running,
            message: None,
        })
    }

    async fn logs(
        &self,
        namespace: String,
        pod_name: String,
        container_name: String,
    ) -> Result<Vec<String>, failure::Error> {
        let mut handles = self.handles.write().await;
        let handle = handles
            .get_mut(&pod_key(&namespace, &pod_name))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod_name.clone(),
            })?
            .get_mut(&container_name)
            .ok_or_else(|| ProviderError::ContainerNotFound {
                pod_name,
                container_name,
            })?;

        Ok((&mut handle.0)
            .lines()
            .map(|l| l.unwrap_or_else(|_| "<error while reading line>".to_string()))
            .collect::<Vec<String>>()
            .await)
    }
}

/// Generates a unique human readable key for storing a handle to a pod
fn key_from_pod(pod: &Pod) -> String {
    pod_key(
        &pod.metadata
            .as_ref()
            .and_then(|m| m.namespace.as_deref())
            .unwrap_or("default"),
        pod.metadata.as_ref().unwrap().name.as_ref().unwrap(),
    )
}

fn pod_key<N: AsRef<str>, T: AsRef<str>>(namespace: N, pod_name: T) -> String {
    format!("{}:{}", namespace.as_ref(), pod_name.as_ref())
}

#[cfg(test)]
mod test {
    use super::*;
    use k8s_openapi::api::core::v1::PodSpec;
    use std::io::Read;

    #[test]
    fn test_can_schedule() {
        let wp = WasiProvider::default();
        let mut mock = Default::default();
        assert!(!wp.can_schedule(&mock));

        let mut selector = std::collections::BTreeMap::new();
        selector.insert(
            "beta.kubernetes.io/arch".to_string(),
            "wasm32-wasi".to_string(),
        );
        mock.spec = Some(PodSpec {
            node_selector: Some(selector.clone()),
            ..Default::default()
        });
        assert!(wp.can_schedule(&mock));
        selector.insert("beta.kubernetes.io/arch".to_string(), "amd64".to_string());
        mock.spec = Some(PodSpec {
            node_selector: Some(selector),
            ..Default::default()
        });
        assert!(!wp.can_schedule(&mock));
    }

    #[test]
    fn test_logs() {
        // TODO: Log testing will need to be done in a full integration test as
        // it requires a kube client
    }
}
