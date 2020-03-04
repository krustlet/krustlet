use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use failure::{bail, format_err};
use kube::client::APIClient;
use kubelet::pod::{pod_status, KubePod};
use kubelet::{Phase, Provider, Status};
use log::{debug, info};
use tempfile::NamedTempFile;
use wasm3::environment::Environment;
use wasm3::module::Module;

const TARGET_WASM32_WASI: &str = "wasm32-wasi";

// PodStore contains a map of a unique pod key pointing to a map of container
// names to the join handle for their running task
type PodStore = HashMap<String, HashMap<String, JoinHandle<()>>>;

/// WasiProvider provides a Kubelet runtime implementation that executes WASM
/// binaries conforming to the WASI spec
#[derive(Clone, Default)]
pub struct Wasm3Provider {
    handles: Arc<RwLock<PodStore>>,
}

impl Provider for Wasm3Provider {
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

        let first_container = pod.spec.containers[0].clone();

        let current_dir = std::env::current_dir()?;
        let env = self.env_vars(client.clone(), &first_container, &pod);

        let runtime = Wasm3Runtime::new(
            PathBuf::from("./testdata/hello-world.wasm"),
            env,
            Vec::default(),
            HashMap::default(),
            current_dir,
        )?;

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
        unimplemented!("cannot stop a running wasmtime instance")
    }
    fn status(&self, _pod: KubePod, _client: APIClient) -> Result<Status, failure::Error> {
        Ok(Status {
            phase: Phase::Running,
            message: None,
        })
    }
    fn logs(&self, _pod: KubePod) -> Result<Vec<String>, failure::Error> {
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
struct Wasm3Runtime {
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

impl Wasm3Runtime {
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
        //let data = include_bytes!("../testdata/hello-world.wasm");
        let data = std::fs::read(module_path)?;

        // Currently unused.
        let stdout = NamedTempFile::new_in(&log_file_location)?;
        let stderr = NamedTempFile::new_in(&log_file_location)?;
        debug!("after files");
        Ok(Wasm3Runtime {
            module_data: data.to_vec(),
            env,
            args,
            dirs,
            stdout,
            stderr,
        })
    }

    fn run(&self) -> Result<(), failure::Error> {
        let env = Environment::new().map_err(|e| format_err!("Ooops, I did it again: {}", e))?;
        // TODO: What are the actual values we want for the runtime size?
        let rt = env
            .create_runtime(1024 * 60)
            .map_err(|e| format_err!("I played with your heart: {}", e))?;
        let data = Module::parse(&env, &self.module_data)
            .map_err(|e| format_err!("I got lost in the game: {}", e))?;
        let mut module = rt
            .load_module(data)
            .map_err(|e| format_err!("Oh baby, baby: {}", e))?;
        module
            .link_wasi()
            .map_err(|e| format_err!("Oops! You think I'm in love: {}", e))?;

        // Right now, there is no way to pass args, env vars, and file handles into
        // the wasm3 runtime in Rust. The module is under active development
        // right now. As it evolves, I'll update this.

        info!("module run starting");
        // Now we call into the WASM modulethread:
        let fun = module
            .find_function::<(), ()>("_start")
            .map_err(|e| format_err!("I'm sent from above: {}", e))?;
        fun.call()
            .map_err(|e| format_err!("I'm not that innocent: {}", e))?;
        info!("module run complete");

        // See http://www.songlyrics.com/britney-spears/oopsi-did-it-again-lyrics/
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

    #[test]
    fn test_can_schedule() {
        let wp = Wasm3Provider::default();
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
        let wr = Wasm3Runtime::new(
            "./testdata/hello-world.wasm",
            HashMap::default(),
            Vec::default(),
            HashMap::default(),
            std::env::temp_dir(),
        )
        .expect("wasi runtime init");
        wr.run().expect("complete run");
        wr.output().expect("process output");

        // Current impl of wasm3_rs writes directly to the FD 1 (in the C shim)
        // So there is no way to intercept the output.
        //let (mut stdout_buf, _) = wr.output().expect("process output");
        //let mut stdout = String::default();
        //stdout_buf.read_to_string(&mut stdout).unwrap();
        //assert_eq!("Hello, world!\n", stdout);
    }
}
