use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, error, info, instrument, trace, warn};

use cap_std::ambient_authority;
use tempfile::NamedTempFile;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use wasi_cap_std_sync::WasiCtxBuilder;
use wasmtime::{InterruptHandle, Linker};

use kubelet::container::Handle as ContainerHandle;
use kubelet::container::Status;
use kubelet::handle::StopHandler;

use wasi_experimental_http_wasmtime::HttpCtx as WasiHttpCtx;

pub struct Runtime {
    handle: JoinHandle<anyhow::Result<()>>,
    interrupt_handle: InterruptHandle,
}

#[async_trait::async_trait]
impl StopHandler for Runtime {
    async fn stop(&mut self) -> anyhow::Result<()> {
        self.interrupt_handle.interrupt();
        Ok(())
    }

    async fn wait(&mut self) -> anyhow::Result<()> {
        (&mut self.handle).await??;
        Ok(())
    }
}

/// WasiRuntime provides a WASI compatible runtime. A runtime should be used for
/// each "instance" of a process and can be passed to a thread pool for running
pub struct WasiRuntime {
    /// name of the process
    name: String,
    /// Data needed for the runtime
    data: Arc<Data>,
    /// The tempfile that output from the wasmtime process writes to
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
        Ok(WasiRuntime {
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

        let (interrupt_handle, handle) = self
            .spawn_wasmtime(tokio::fs::File::from_std(output_write))
            .await?;

        let log_handle_factory = HandleFactory {
            temp: self.output.clone(),
        };

        Ok(ContainerHandle::new(
            Runtime {
                handle,
                interrupt_handle,
            },
            log_handle_factory,
        ))
    }

    // Spawns a running wasmtime instance with the given context and status
    // channel.
    #[instrument(level = "info", skip(self, output_write), fields(name = %self.name))]
    async fn spawn_wasmtime(
        &self,
        output_write: tokio::fs::File,
    ) -> anyhow::Result<(InterruptHandle, JoinHandle<anyhow::Result<()>>)> {
        // Clone the module data Arc so it can be moved
        let data = self.data.clone();
        let status_sender = self.status_sender.clone();

        // Log this info here so it isn't on _every_ log line
        trace!(env = ?data.env, args = ?data.args, dirs = ?data.dirs, "Starting setup of wasmtime module");
        let env: Vec<(String, String)> = data
            .env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let stdout = wasi_cap_std_sync::file::File::from_cap_std(cap_std::fs::File::from_std(
            output_write.try_clone().await?.into_std().await,
            ambient_authority(),
        ));
        let stderr = wasi_cap_std_sync::file::File::from_cap_std(cap_std::fs::File::from_std(
            output_write.try_clone().await?.into_std().await,
            ambient_authority(),
        ));

        // Create the WASI context builder and pass arguments, environment,
        // and standard output and error.
        let mut builder = WasiCtxBuilder::new()
            .args(&data.args)?
            .envs(&env)?
            .stdout(Box::new(stdout))
            .stderr(Box::new(stderr));

        // Add preopen dirs.
        for (key, value) in data.dirs.iter() {
            let guest_dir = value.as_ref().unwrap_or(key);
            debug!(
                hostpath = %key.display(),
                guestpath = %guest_dir.display(),
                "mounting hostpath in modules"
            );
            let preopen_dir = cap_std::fs::Dir::open_ambient_dir(key, ambient_authority())?;

            builder = builder.preopened_dir(preopen_dir, guest_dir)?;
        }

        let ctx = builder.build();

        let mut config = wasmtime::Config::new();
        config.interruptable(true);
        let engine = wasmtime::Engine::new(&config)?;
        let mut store = wasmtime::Store::new(&engine, ctx);
        let interrupt = store.interrupt_handle()?;

        let mut linker = Linker::new(&engine);

        let module = match wasmtime::Module::new(&engine, &data.module_data) {
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

        wasmtime_wasi::add_to_linker(&mut linker, |cx| cx)?;

        // Link WASI HTTP
        let WasiHttpConfig {
            allowed_domains,
            max_concurrent_requests,
        } = self.http_config.clone();
        let wasi_http = WasiHttpCtx::new(allowed_domains, max_concurrent_requests)?;
        wasi_http.add_to_linker(&mut linker)?;

        let instance = match linker.instantiate(&mut store, &module) {
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

        info!("starting run of module");
        status_sender
            .send(Status::Running {
                timestamp: chrono::Utc::now(),
            })
            .await?;

        // NOTE(thomastaylor312): In the future, if we want to pass args directly, we'll
        // need to do a bit more to pass them in here.
        let export = instance
            .get_export(&mut store, "_start")
            .ok_or_else(|| anyhow::anyhow!("_start import doesn't exist in wasm module"))?;

        // NOTE(thomastaylor312): In the future (pun intended) we might be able to use something
        // like `func.call(...).await`. We should check every once and a while when upgraing
        // wasmtime
        let func = match export {
            wasmtime::Extern::Func(f) => f,
            _ => {
                let message =
                    "_start import was not a function. This is likely a problem with the module";
                error!(error = message);
                status_sender
                    .send(Status::Terminated {
                        failed: true,
                        message: message.into(),
                        timestamp: chrono::Utc::now(),
                    })
                    .await?;

                return Err(anyhow::anyhow!(message));
            }
        };

        let name = self.name.clone();
        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let span = tracing::info_span!("wasmtime_module_run", %name);
            let _enter = span.enter();

            match func.call(&mut store, &[]) {
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
        Ok((interrupt, handle))
    }
}

#[instrument(level = "info", skip(sender, status))]
fn send(sender: &Sender<Status>, name: &str, status: Status) {
    match sender.blocking_send(status) {
        Err(e) => warn!(error = %e, "error sending wasi status"),
        Ok(_) => debug!("send completed"),
    }
}
