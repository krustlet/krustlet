use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, error, info, instrument, trace, warn};

use tempfile::NamedTempFile;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

use kubelet::container::Handle as ContainerHandle;
use kubelet::container::Status;
use kubelet::handle::StopHandler;
use wasi_experimental_http_wasmer::HttpCtx as WasmerHttpCtx;
use wasmer::{Cranelift, Instance, Module, Store, Universal};
use wasmer_wasi::WasiState;

use crate::pipes::FilePipe;

// TODO: implement it for wasmer
// use wasi_experimental_http_wasmer::HttpCtx as WasiHttpCtx;

pub struct Runtime {
    handle: JoinHandle<anyhow::Result<()>>,
    // interrupt_handle: InterruptHandle,
}

#[async_trait::async_trait]
impl StopHandler for Runtime {
    async fn stop(&mut self) -> anyhow::Result<()> {
        // self.interrupt_handle.interrupt();
        // TODO implement interruption of wasmer
        Ok(())
    }

    async fn wait(&mut self) -> anyhow::Result<()> {
        (&mut self.handle).await??;
        Ok(())
    }
}

/// WasmerRuntime provides a WASI compatible runtime with wasmer. A runtime should be used for
/// each "instance" of a process and can be passed to a thread pool for running
pub struct WasmerRuntime {
    /// name of the process
    name: String,
    /// Data needed for the runtime
    data: Arc<Data>,
    /// The tempfile that output from the wasmer process writes to
    output: Arc<NamedTempFile>,
    /// A channel to send status updates on the runtime
    status_sender: Sender<Status>,
    /// Configuration for the WASI http
    http_config: WasiHttpConfig,
}

// Configuration for WASI http.
#[derive(Clone, Default)]
pub struct WasiHttpConfig {
    pub allowed_domains: Option<Vec<String>>,
    pub max_concurrent_requests: Option<u32>,
}

struct Data {
    /// binary module data to be run as a wasm module
    module_data: Vec<u8>,
    /// key/value environment variables made available to the wasm process
    env: HashMap<String, String>,
    /// the arguments passed as the command-line arguments list
    args: Vec<String>,
    /// a hash map of local file system paths to optional path names in the runtime
    /// (e.g. /tmp/foo/myfile -> /app/config). If the optional value is not given,
    /// the same path will be allowed in the runtime
    dirs: HashMap<PathBuf, Option<PathBuf>>,
}

/// Holds our tempfile handle.
pub struct HandleFactory {
    temp: Arc<NamedTempFile>,
}

impl kubelet::log::HandleFactory<tokio::fs::File> for HandleFactory {
    /// Creates `tokio::fs::File` on demand for log reading.
    fn new_handle(&self) -> tokio::fs::File {
        tokio::fs::File::from_std(self.temp.reopen().unwrap())
    }
}

impl WasmerRuntime {
    /// Creates a new WasmerRuntime
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
    #[allow(clippy::too_many_arguments)]
    pub async fn new<L: AsRef<Path> + Send + Sync + 'static>(
        name: String,
        module_data: Vec<u8>,
        env: HashMap<String, String>,
        args: Vec<String>,
        dirs: HashMap<PathBuf, Option<PathBuf>>,
        log_dir: L,
        status_sender: Sender<Status>,
        http_config: WasiHttpConfig,
    ) -> anyhow::Result<Self> {
        let temp = tokio::task::spawn_blocking(move || -> anyhow::Result<NamedTempFile> {
            Ok(NamedTempFile::new_in(log_dir)?)
        })
        .await??;

        // We need to use named temp file because we need multiple file handles
        // and if we are running in the temp dir, we run the possibility of the
        // temp file getting cleaned out from underneath us while running. If we
        // think it necessary, we can make these permanent files with a cleanup
        // loop that runs elsewhere. These will get deleted when the reference
        // is dropped
        Ok(WasmerRuntime {
            name,
            data: Arc::new(Data {
                module_data,
                env,
                args,
                dirs,
            }),
            output: Arc::new(temp),
            status_sender,
            http_config,
        })
    }

    pub async fn start(&self) -> anyhow::Result<ContainerHandle<Runtime, HandleFactory>> {
        let temp = self.output.clone();
        // Because a reopen is blocking, run in a blocking task to get new
        // handles to the tempfile
        let output_write = tokio::task::spawn_blocking(move || -> anyhow::Result<std::fs::File> {
            Ok(temp.reopen()?)
        })
        .await??;

        let handle = self
            .spawn_wasmer(tokio::fs::File::from_std(output_write))
            .await?;

        let log_handle_factory = HandleFactory {
            temp: self.output.clone(),
        };

        Ok(ContainerHandle::new(
            Runtime {
                handle,
                // interrupt_handle,
            },
            log_handle_factory,
        ))
    }

    // Spawns a running wasmer instance with the given context and status
    // channel.
    #[instrument(level = "info", skip(self, output_write), fields(name = %self.name))]
    async fn spawn_wasmer(
        &self,
        output_write: tokio::fs::File,
    ) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
        // TODO: add wasmer implementation of wasi-experimental-http and use it here
        // Clone the module data Arc so it can be moved
        let data = self.data.clone();
        let status_sender = self.status_sender.clone();

        // Log this info here so it isn't on _every_ log line
        trace!(env = ?data.env, args = ?data.args, dirs = ?data.dirs, "Starting setup of wasmer module");
        let env: Vec<(String, String)> = data
            .env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Add preopen dirs.
        let mapped_dirs = data.dirs.iter().map(|(host_path, guest_path)| {
            (
                guest_path
                    .clone()
                    .unwrap_or_else(|| host_path.clone())
                    .to_string_lossy()
                    .to_string(),
                host_path,
            )
        });
        let store = Store::new(&Universal::new(Cranelift::default()).engine());
        let module = match Module::new(&store, &data.module_data) {
            // We can't map errors here or it moves the send channel, so we
            // do it in a match
            Ok(m) => m,
            Err(e) => {
                let message = "unable to create module";
                error!(error = %e, "{}", message);
                status_sender
                    .send(Status::Terminated {
                        failed: true,
                        message: message.into(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await?;

                return Err(anyhow::anyhow!("{}: {}", message, e));
            }
        };
        let stderr = FilePipe::try_from(output_write.try_clone().await?)?;
        let stdout = FilePipe::try_from(output_write)?;
        let mut wasi_env = WasiState::new(&self.name)
            .stdout(Box::new(stdout))
            .stderr(Box::new(stderr))
            .map_dirs(mapped_dirs)?
            .envs(env)
            .finalize()?;

        println!("Instantiating module with WASI imports...");
        // Then, we get the import object related to our WASI
        // and attach it to the Wasm instance.
        let mut import_object = match wasi_env.import_object(&module) {
            Ok(i) => i,
            Err(e) => {
                let message = "unable to import object";
                error!(error = %e, "{}", message);
                status_sender
                    .send(Status::Terminated {
                        failed: true,
                        message: message.into(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await?;
                // Converting from anyhow
                return Err(anyhow::anyhow!("{}: {}", message, e));
            }
        };

        println!("HTTP Config");
        // Link WASI HTTP
        let WasiHttpConfig {
            allowed_domains,
            max_concurrent_requests,
        } = self.http_config.clone();
        let wasi_http = WasmerHttpCtx::new(allowed_domains, max_concurrent_requests)?;
        wasi_http.add_to_import_object(&store, wasi_env, &mut import_object)?;
        println!("HTTP Config imported");

        let instance = match Instance::new(&module, &import_object) {
            // We can't map errors here or it moves the send channel, so we
            // do it in a match
            Ok(i) => i,
            Err(e) => {
                let message = "unable to instantiate module";
                error!(error = %e, "{}", message);
                status_sender
                    .send(Status::Terminated {
                        failed: true,
                        message: message.into(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await?;
                // Converting from anyhow
                return Err(anyhow::anyhow!("{}: {}", message, e));
            }
        };
        let memory = instance.exports.get_memory("memory")?;
        // println!("------ memory {:?}", unsafe { memory.data_unchecked() });

        println!("starting run of module");
        info!("starting run of module");
        status_sender
            .send(Status::Running {
                timestamp: chrono::Utc::now(),
            })
            .await?;
        let name = self.name.clone();
        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let func = instance
                .exports
                .get_function("_start")
                .map_err(|_| anyhow::anyhow!("_start import doesn't exist in wasm module"))?;

            let span = tracing::info_span!("wasmer_module_run", %name);
            let _enter = span.enter();

            println!("CALLING");
            match func.call(&[]) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(_) => {}
                Err(e) => {
                    let message = "unable to run module";
                    error!(error = %e, "{}", message);
                    send(
                        &status_sender,
                        &name,
                        Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        },
                    );

                    return Err(anyhow::anyhow!("{}: {}", message, e));
                }
            };

            info!("module run complete");
            send(
                &status_sender,
                &name,
                Status::Terminated {
                    failed: false,
                    message: "Module run completed".into(),
                    timestamp: chrono::Utc::now(),
                },
            );
            Ok(())
        });
        // Wait for the interrupt to be sent back to us
        Ok(handle)
    }
}

#[instrument(level = "info", skip(sender, status))]
fn send(sender: &Sender<Status>, name: &str, status: Status) {
    match sender.blocking_send(status) {
        Err(e) => warn!(error = %e, "error sending wasmer status"),
        Ok(_) => debug!("send completed"),
    }
}
