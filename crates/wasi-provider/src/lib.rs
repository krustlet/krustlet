#[macro_use]
extern crate failure;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;

use kube::client::APIClient;
use kubelet::pod::{pod_status, KubePod};
use kubelet::{Phase, Provider, Status};
use log::{debug, info};
use std::thread::JoinHandle;
use tempfile::NamedTempFile;
use wasi_common::*;
use wasmtime::*;
use wasmtime_wasi::old::snapshot_0::Wasi as WasiUnstable;
use wasmtime_wasi::*;

const TARGET_WASM32_WASI: &str = "wasm32-wasi";

// PodStore contains a map of a unique pod key pointing to a map of container
// names to the join handle for their running task
type PodStore = HashMap<String, HashMap<String, JoinHandle<()>>>;

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec
#[derive(Clone, Default)]
pub struct WasiProvider {
    handles: Arc<RwLock<PodStore>>,
}

impl Provider for WasiProvider {
    fn init(&self) -> Result<(), failure::Error> {
        Ok(())
    }

    fn arch(&self) -> String {
        TARGET_WASM32_WASI.to_string()
    }

    fn can_schedule(&self, pod: &KubePod) -> bool {
        // If there is a node selector and it has arch set to wasm32-wasi, we can
        // schedule it.
        pod.spec
            .node_selector
            .as_ref()
            .and_then(|i| {
                i.get("beta.kubernetes.io/arch")
                    .map(|v| v.eq(&TARGET_WASM32_WASI))
            })
            .unwrap_or(false)
    }
    fn add(&self, pod: KubePod, client: APIClient) -> Result<(), failure::Error> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.
        debug!("Pod added {:?}", pod.metadata.name);
        let namespace = pod
            .metadata
            .clone()
            .namespace
            .unwrap_or_else(|| "default".into());

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
        let first_container = pod.spec.containers[0].clone();

        // TODO: Actual log path configuration
        let current_dir = std::env::current_dir()?;
        let env = self.env_vars(client.clone(), &first_container, &pod);

        // TODO: Replace with actual image store lookup when it is merged
        let runtime = WasiRuntime::new(
            PathBuf::from("./testdata/hello-world.wasm"),
            env,
            Vec::default(),
            HashMap::default(),
            current_dir,
        )?;

        //let args = first_container.args.unwrap_or_else(|| vec![]);
        // TODO: we shoult probably make the Providers and kubelet async and use tokio like so:
        // let handle = tokio::spawn(async move {
        //     tokio::task::spawn_blocking(move || {
        //         runtime.run()?;
        //         Ok::<(), failure::Error>(())
        //     })
        //     .await?
        // });
        let handle = std::thread::spawn(move || runtime.run().unwrap());
        let mut handles = self.handles.write().unwrap();
        handles
            .entry(pod_key(&pod))
            .or_default()
            .insert(first_container.name, handle);
        info!("Pod is executing on a thread");
        pod_status(client, pod, "Running", namespace.as_str());
        Ok(())
    }
    fn modify(&self, pod: KubePod, _client: APIClient) -> Result<(), failure::Error> {
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
    fn delete(&self, _pod: KubePod, _client: APIClient) -> Result<(), failure::Error> {
        // There is currently no way to stop a long running instance, so we are
        // SOL here until there is support for it. See
        // https://github.com/bytecodealliance/wasmtime/issues/860 for more
        // information
        unimplemented!("cannot stop a running wasmtime instance")
    }
    fn status(&self, _pod: KubePod, _client: APIClient) -> Result<Status, failure::Error> {
        // TODO(taylor): Figure out the best way to check if a future is still
        // running. I get the feeling that manually calling `poll` on the future
        // is a Bad Idea™ and so I am not sure if there is another way or if we
        // should implement messaging using channels to let the main runtime
        // know it is done
        // let fut = async {
        //     let handles = self.handles.read().await;
        //     let containers = handles.get(pod_key(&pod));
        // };
        Ok(Status {
            phase: Phase::Running,
            message: None,
        })
    }
    fn logs(&self, _pod: KubePod) -> Result<Vec<String>, failure::Error> {
        // TODO: We'll need to probably pass the runtime, output method, or a
        // combined stream of stdout and stderr into our hashmap so we can
        // expose them here
        unimplemented!()
    }
}

/// Generates a unique human readable key for storing a handle to a pod
fn pod_key(pod: &KubePod) -> String {
    format!(
        "{}:{}",
        pod.metadata
            .namespace
            .clone()
            .unwrap_or_else(|| "default".into()),
        pod.metadata.name
    )
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
    /// Handle to stdout
    stdout: NamedTempFile,
    /// handle to stderr
    stderr: NamedTempFile,
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
    pub fn new<M: AsRef<Path>, L: AsRef<Path>>(
        module_path: M,
        env: HashMap<String, String>,
        args: Vec<String>,
        dirs: HashMap<String, Option<String>>,
        log_file_location: L,
    ) -> Result<Self, failure::Error> {
        debug!("before parse file: {:?}", std::env::current_dir().unwrap());
        let module_data = wat::parse_file(module_path)?;
        debug!("after parse file");

        // We need to use named temp file because we need multiple file handles
        // and if we are running in the temp dir, we run the possibility of the
        // temp file getting cleaned out from underneath us while running. If we
        // think it necessary, we can make these permanent files with a cleanup
        // loop that runs elsewhere. These will get deleted when the reference
        // is dropped
        let stdout = NamedTempFile::new_in(&log_file_location)?;
        let stderr = NamedTempFile::new_in(&log_file_location)?;
        debug!("after files");
        Ok(WasiRuntime {
            module_data,
            env,
            args,
            dirs,
            stdout,
            stderr,
        })
    }

    fn run(&self) -> Result<(), failure::Error> {
        let engine = Engine::default();
        let store = Store::new(&engine);

        // Build the WASI instance and then generate a list of WASI modules
        let mut ctx_builder_snapshot = WasiCtxBuilder::new()
            .args(&self.args)
            .envs(&self.env)
            .stdout(self.stdout.reopen()?)
            .stderr(self.stderr.reopen()?);
        let mut ctx_builder_unstable = wasi_common::old::snapshot_0::WasiCtxBuilder::new()
            .args(&self.args)
            .envs(&self.env)
            .stdout(self.stdout.reopen()?)
            .stderr(self.stderr.reopen()?);

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
        let module = Module::new(&store, &self.module_data)
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

        let instance = Instance::new(&module, &imports)
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

    /// output returns a tuple of BufReaders containing stdout and stderr
    /// respectively. It will error if it can't open a stream
    // TODO(taylor): I can't completely tell from documentation, but we may
    // need to switch this out from a BufReader if it can't handle streaming
    // logs
    fn output(&self) -> Result<(BufReader<File>, BufReader<File>), failure::Error> {
        // As warned in the BufReader docs, creating multiple BufReaders on the
        // same stream can cause data loss. So reopen a new file object each
        // time this function as called so as to not drop any data
        Ok((
            BufReader::new(self.stdout.reopen()?),
            BufReader::new(self.stderr.reopen()?),
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use k8s_openapi::api::core::v1::PodSpec;
    use kubelet::pod::KubePod;
    use std::io::Read;

    #[test]
    fn test_can_schedule() {
        let wp = WasiProvider::default();
        let mut mock = KubePod {
            spec: Default::default(),
            metadata: Default::default(),
            status: Default::default(),
            types: Default::default(),
        };
        assert!(!wp.can_schedule(&mock));

        let mut selector = std::collections::BTreeMap::new();
        selector.insert(
            "beta.kubernetes.io/arch".to_string(),
            "wasm32-wasi".to_string(),
        );
        mock.spec = PodSpec {
            node_selector: Some(selector.clone()),
            ..Default::default()
        };
        assert!(wp.can_schedule(&mock));
        selector.insert("beta.kubernetes.io/arch".to_string(), "amd64".to_string());
        mock.spec = PodSpec {
            node_selector: Some(selector),
            ..Default::default()
        };
        assert!(!wp.can_schedule(&mock));
    }

    #[test]
    fn test_run() {
        let wr = WasiRuntime::new(
            "./testdata/hello-world.wasm",
            HashMap::default(),
            Vec::default(),
            HashMap::default(),
            std::env::temp_dir(),
        )
        .expect("wasi runtime init");
        wr.run().expect("complete run");
        let (mut stdout_buf, _) = wr.output().expect("process output");
        let mut stdout = String::default();

        stdout_buf.read_to_string(&mut stdout).unwrap();
        assert_eq!("Hello, world!\n", stdout);
    }
}
