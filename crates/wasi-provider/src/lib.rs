use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use failure::{bail, format_err};
use kube::client::APIClient;
use kubelet::pod::{pod_status, Pod};
use kubelet::{Phase, Provider, ProviderError, Status};
use log::{debug, info};
use tempfile::NamedTempFile;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use wasi_common::preopen_dir;
use wasmtime_wasi::old::snapshot_0::Wasi as WasiUnstable;
use wasmtime_wasi::{Wasi, WasiCtxBuilder};

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

        // TODO: Actual log path configuration
        let tempfile = NamedTempFile::new_in(std::env::current_dir()?)?;
        // Get a separate file handle that can be moved onto the thread
        let output = tempfile.reopen()?;

        let handle = tokio::task::spawn_blocking(move || runtime.run(output).unwrap());
        {
            let mut handles = self.handles.write().await;
            handles.entry(key_from_pod(&pod)).or_default().insert(
                first_container.name,
                (BufReader::new(tempfile.reopen()?), handle),
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
        pod: String,
        container: String,
    ) -> Result<Vec<String>, failure::Error> {
        let mut handles = self.handles.write().await;
        let handle = handles
            .get_mut(&pod_key(&namespace, &pod))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod.clone(),
            })?
            .get_mut(&container)
            .ok_or_else(|| ProviderError::ContainerNotFound {
                pod_name: pod,
                container_name: container,
            })?;
        Ok(handle
            .0
            .by_ref()
            .lines()
            .map(|l| l.unwrap_or_else(|_| "<error while reading line>".to_string()))
            .collect::<Vec<String>>())
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

/// WasiRuntime provides a WASI compatible runtime. A runtime should be used for
/// each "instance" of a process and can be passed to a thread pool for running
struct WasiRuntime {
    /// binary module data to be run as a wasm module
    module_data: Vec<u8>,
    /// key/value environment variables made available to the wasm process
    env: HashMap<String, String>,
    /// the arguments passed as the command-line arguments list
    args: Vec<String>,
    /// a hash map of local file system paths to optional path names in the runtime
    /// (e.g. /tmp/foo/myfile -> /app/config). If the optional value is not given,
    /// the same path will be allowed in the runtime
    dirs: HashMap<String, Option<String>>,
}

impl WasiRuntime {
    /// Creates a new WasiRuntime
    ///
    /// # Arguments
    ///
    /// * `module_path` - the path to the WebAssembly binary
    /// * `env` - a collection of key/value pairs containing the environment variables
    /// * `args` - the arguments passed as the command-line arguments list
    /// * `dirs` - a map of local file system paths to optional path names in the runtime
    ///     (e.g. /tmp/foo/myfile -> /app/config). If the optional value is not given,
    ///     the same path will be allowed in the runtime
    /// * `log_file_location` - location for storing logs
    pub fn new<M: AsRef<Path>>(
        module_path: M,
        env: HashMap<String, String>,
        args: Vec<String>,
        dirs: HashMap<String, Option<String>>,
    ) -> Result<Self, failure::Error> {
        let module_data = wat::parse_file(module_path)?;

        // We need to use named temp file because we need multiple file handles
        // and if we are running in the temp dir, we run the possibility of the
        // temp file getting cleaned out from underneath us while running. If we
        // think it necessary, we can make these permanent files with a cleanup
        // loop that runs elsewhere. These will get deleted when the reference
        // is dropped
        Ok(WasiRuntime {
            module_data,
            env,
            args,
            dirs,
        })
    }

    fn run(&self, output: File) -> Result<(), failure::Error> {
        let engine = wasmtime::Engine::default();
        let store = wasmtime::Store::new(&engine);

        // Build the WASI instance and then generate a list of WASI modules
        let mut ctx_builder_snapshot = WasiCtxBuilder::new();
        // For some reason if I didn't split these out, the compiler got mad
        let mut ctx_builder_snapshot = ctx_builder_snapshot
            .args(&self.args)
            .envs(&self.env)
            .stdout(output.try_clone()?)
            .stderr(output.try_clone()?);
        let mut ctx_builder_unstable = wasi_common::old::snapshot_0::WasiCtxBuilder::new()
            .args(&self.args)
            .envs(&self.env)
            .stdout(output.try_clone()?)
            .stderr(output);

        for (key, value) in self.dirs.iter() {
            let guest_dir = value.as_ref().unwrap_or(key);
            // Try and preopen the directory and then try to clone it. This step adds the directory to the context
            ctx_builder_snapshot = ctx_builder_snapshot.preopened_dir(preopen_dir(key)?, guest_dir);
            ctx_builder_unstable = ctx_builder_unstable.preopened_dir(preopen_dir(key)?, guest_dir);
        }
        let wasi_ctx_snapshot = ctx_builder_snapshot.build()?;
        let wasi_ctx_unstable = ctx_builder_unstable.build()?;

        let wasi_snapshot = Wasi::new(&store, wasi_ctx_snapshot);
        let wasi_unstable = WasiUnstable::new(&store, wasi_ctx_unstable);
        let module = wasmtime::Module::new(&store, &self.module_data)
            .map_err(|e| format_err!("unable to load module data {}", e))?;
        // Iterate through the module includes and resolve imports
        let imports = module
            .imports()
            .iter()
            .map(|i| {
                // This is super funky logic, but it matches what is in 0.11.0
                let export = match i.module() {
                    "wasi_snapshot_preview1" => wasi_snapshot.get_export(i.name()),
                    "wasi_unstable" => wasi_unstable.get_export(i.name()),
                    other => bail!("import module `{}` was not found", other),
                };
                match export {
                    Some(export) => Ok(export.clone().into()),
                    None => bail!(
                        "import `{}` was not found in module `{}`",
                        i.name(),
                        i.module()
                    ),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        let instance = wasmtime::Instance::new(&module, &imports)
            .map_err(|e| format_err!("unable to instantiate module: {}", e))?;

        // NOTE(taylor): In the future, if we want to pass args directly, we'll
        // need to do a bit more to pass them in here.
        info!("starting run of module");
        instance
            .get_export("_start")
            .expect("export")
            .func()
            .unwrap()
            .call(&[])
            .unwrap();

        info!("module run complete");
        Ok(())
    }
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
    fn test_run() {
        let wr = WasiRuntime::new(
            "./testdata/hello-world.wasm",
            HashMap::default(),
            Vec::default(),
            HashMap::default(),
        )
        .expect("wasi runtime init");
        let output = NamedTempFile::new().unwrap();
        wr.run(output.reopen().unwrap()).expect("complete run");
        let mut stdout = String::default();

        let mut output = BufReader::new(output);
        output.read_to_string(&mut stdout).unwrap();
        assert_eq!("Hello, world!\n", stdout);
    }

    #[test]
    fn test_logs() {
        // TODO: Log testing will need to be done in a full integration test as
        // it requires a kube client
    }
}
