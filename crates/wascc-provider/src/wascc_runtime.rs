use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use log::{debug, error, info, warn};
use tempfile::NamedTempFile;
use tokio::sync::watch::{self, Sender};
use tokio::task::JoinHandle;

use std::path::{Path, PathBuf};

use wascc_host::{host, Actor, NativeCapability};

use kubelet::handle::{RuntimeHandle, Stop};
use kubelet::status::ContainerStatus;
use wascc_logging::LOG_PATH_KEY;

/// The name of the HTTP capability.
const HTTP_CAPABILITY: &str = "wascc:http_server";
const LOG_CAPABILITY: &str = "wascc:logging";

#[cfg(target_os = "linux")]
const HTTP_LIB: &str = "./lib/libwascc_httpsrv.so";

#[cfg(target_os = "linux")]
const LOG_LIB: &str = "./lib/libwascc_logging.so";

#[cfg(target_os = "macos")]
const HTTP_LIB: &str = "./lib/libwascc_httpsrv.dylib";

#[cfg(target_os = "macos")]
const LOG_LIB: &str = "./lib/libwascc_logging.dylib";

pub struct ActorStopper {
    pub key: String,
}

#[async_trait::async_trait]
impl Stop for ActorStopper {
    async fn stop(&mut self) -> anyhow::Result<()> {
        debug!("stopping wascc instance {}", self.key);
        host::remove_actor(self.key)
    }

    async fn wait(&mut self) -> anyhow::Result<()> {
        // TODO: Figure out if there is a way to wait for an actor to be removed
        Ok(())
    }
}

/// WasccRuntime provides a waSCC compatible runtime. A runtime should be used for
/// each "instance" of a process and can be passed to a thread pool for running
pub struct WasccRuntime {
    /// binary module data to be run as a wasm module
    module_data: Arc<Vec<u8>>,
    /// key/value environment variables made available to the wasm process
    env: HashMap<String, String>,
    log_path: PathBuf,
    /// The tempfile that output from the wasmtime process writes to
    output: Arc<NamedTempFile>,
}

impl WasccRuntime {
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
    /// * `log_dir` - location for storing logs
    pub async fn new<L: AsRef<Path> + Send + Sync + 'static>(
        module_data: Vec<u8>,
        env: HashMap<String, String>,
        log_path: PathBuf,
        log_dir: L,
    ) -> anyhow::Result<Self> {
        let temp = tokio::task::spawn_blocking(move || -> anyhow::Result<NamedTempFile> {
            Ok(NamedTempFile::new_in(log_path)?)
        })
        .await??;

        tokio::task::spawn_blocking(|| {
            debug!("wascc: Loading HTTP Capability");
            let data = NativeCapability::from_file(HTTP_LIB).map_err(|e| {
                anyhow::anyhow!("Failed to read HTTP capability {}: {}", HTTP_LIB, e)
            })?;
            host::add_native_capability(data)
                .map_err(|e| anyhow::anyhow!("Failed to load HTTP capability: {}", e))?;

            debug!("wascc: Loading LOG Capability");
            let logdata = NativeCapability::from_file(LOG_LIB)
                .map_err(|e| anyhow::anyhow!("Failed to read LOG capability {}: {}", LOG_LIB, e))?;
            host::add_native_capability(logdata)
                .map_err(|e| anyhow::anyhow!("Failed to load LOG capability: {}", e))
        })
        .await??;
        // We need to use named temp file because we need multiple file handles
        // and if we are running in the temp dir, we run the possibility of the
        // temp file getting cleaned out from underneath us while running. If we
        // think it necessary, we can make these permanent files with a cleanup
        // loop that runs elsewhere. These will get deleted when the reference
        // is dropped
        Ok(WasccRuntime {
            module_data: Arc::new(module_data),
            env,
            log_path,
            output: Arc::new(temp),
        })
    }

    pub async fn start(&self) -> anyhow::Result<RuntimeHandle<tokio::fs::File, HandleStopper>> {
        let temp = self.output.clone();
        // Because a reopen is blocking, run in a blocking task to get new
        // handles to the tempfile
        let (output_write, output_read) = tokio::task::spawn_blocking(
            move || -> anyhow::Result<(std::fs::File, std::fs::File)> {
                Ok((temp.reopen()?, temp.reopen()?))
            },
        )
        .await??;

        let (status_sender, status_recv) = watch::channel(ContainerStatus::Waiting {
            timestamp: chrono::Utc::now(),
            message: "No status has been received from the process".into(),
        });
        let handle = self.spawn_wascc(self.module_data.to_vec(),self.env.clone(),status_sender).await;

        Ok(RuntimeHandle::new(
            tokio::fs::File::from_std(output_read),
            HandleStopper { handle },
            status_recv,
        ))
    }

    /// Stop a running waSCC actor.
    pub fn stop(key: &str) -> anyhow::Result<(), wascc_host::errors::Error> {
        host::remove_actor(key)
    }
    

    // Spawns a running wasmtime instance with the given context and status
    // channel. Due to the Instance type not being Send safe, all of the logic
    // needs to be done within the spawned task
    async fn spawn_wascc(
        &self,
        data: Vec<u8>,
        env: HashMap<String,String>,
        status_sender: Sender<ContainerStatus>,
    ) -> anyhow::Result<()> {
        // Clone the module data Arc so it can be moved

        let mut caps: Vec<Capability> = Vec::new();

        caps.push(Capability {
            name: HTTP_CAPABILITY,
            env: env.clone(),
        });

        let load = Actor::from_bytes(data.to_vec())
            .map_err(|e| anyhow::anyhow!("Error loading WASM: {}", e))
            .unwrap();
        let pk = load.public_key();

        //.unwrap()  self.wascc_run(module_data.to_vec(), &pk, &mut caps).map_err(|e| anyhow::anyhow!("Error loading WASM: {}", e)).unwrap();
        let load =
        Actor::from_bytes(data).map_err(|e| anyhow::anyhow!("Error loading WASM: {}", e)).unwrap();
        let pk = load.public_key();

        let mut logenv: HashMap<String, String> = HashMap::new();
        let actor_log_path = self.log_path.join(pk.clone()).join("log.txt");
        tokio::fs::create_dir_all(&actor_log_path).await
                .map_err(|e| anyhow::anyhow!("error creating directory: {}", e))?;
        logenv.insert(
            LOG_PATH_KEY.to_string(),
            actor_log_path.to_str().unwrap().to_owned(),
        );
        
        caps.push(Capability {
            name: LOG_CAPABILITY,
            env: logenv,
        });

        info!("beginning wascc run for: {}", pk);
        
        host::add_actor(load).map_err(|e| anyhow::anyhow!("Error adding actor: {}", e))?;

        caps.iter().try_for_each(|cap| {
            info!("configuring capability {}", cap.name);
            host::configure(&pk, cap.name, cap.env.clone())
                .map_err(|e| anyhow::anyhow!("Error configuring capabilities for module: {}", e))
        })?;
        info!("Instance executing");
        info!("module run complete");
        status_sender
            .broadcast(ContainerStatus::Terminated {
                failed: false,
                message: "Module run completed".into(),
                timestamp: chrono::Utc::now(),
            })
            .expect("status should be able to send");
        Ok(())
    }
}

/// Capability describes a waSCC capability.
///
/// Capabilities are made available to actors through a two-part processthread:
/// - They must be registered
/// - For each actor, the capability must be configured
struct Capability {
    name: &'static str,
    env: EnvVars,
}

/// Kubernetes' view of environment variables is an unordered map of string to string.
type EnvVars = std::collections::HashMap<String, String>;
/*


let http_result = tokio::task::spawn_blocking(move || {
               wascc_run_http(module_data, env, &pub_key, &lp)
           })
           .await?;
           match http_result {
               Ok(_) => {
                   let mut container_statuses = HashMap::new();
                   container_statuses.insert(
                       container.name.clone(),
                       ContainerStatus::Running {
                           timestamp: chrono::Utc::now(),
                       },
                   );
                   let status = Status {
                       container_statuses,
                       ..Default::default()
                   };
                   pod.patch_status(client.clone(), status).await;
               }
               Err(e) => {
                   let mut container_statuses = HashMap::new();
                   container_statuses.insert(
                       container.name.clone(),
                       ContainerStatus::Terminated {
                           timestamp: chrono::Utc::now(),
                           failed: true,
                           message: "Error while starting container".to_string(),
                       },
                   );
                   let status = Status {
                       container_statuses,
                       ..Default::default()
                   };
                   pod.patch_status(client, status).await;
                   return Err(anyhow::anyhow!("Failed to run pod: {}", e));
               }
           }
       }
let http_result = tokio::task::spawn_blocking(move || {
               wascc_run_http(module_data, env, &pub_key, &lp)
           })
           .await?;
           match http_result {
               Ok(_) => {
                   let mut container_statuses = HashMap::new();
                   container_statuses.insert(
                       container.name.clone(),
                       ContainerStatus::Running {
                           timestamp: chrono::Utc::now(),
                       },
                   );
                   let status = Status {
                       container_statuses,
                       ..Default::default()
                   };
                   pod.patch_status(client.clone(), status).await;
               }
               Err(e) => {
                   let mut container_statuses = HashMap::new();
                   container_statuses.insert(
                       container.name.clone(),
                       ContainerStatus::Terminated {
                           timestamp: chrono::Utc::now(),
                           failed: true,
                           message: "Error while starting container".to_string(),
                       },
                   );
                   let status = Status {
                       container_statuses,
                       ..Default::default()
                   };
                   pod.patch_status(client, status).await;
                   return Err(anyhow::anyhow!("Failed to run pod: {}", e));
               }
           }
       }
       */
