use anyhow::bail;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use tempfile::NamedTempFile;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use wasi_cap_std_sync::WasiCtxBuilder;
use wasmtime::InterruptHandle;
use wasmtime_wasi::snapshots::preview_0::Wasi as WasiUnstable;
use wasmtime_wasi::snapshots::preview_1::Wasi;

use kubelet::container::Handle as ContainerHandle;
use kubelet::container::Status;
use kubelet::handle::StopHandler;

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
    // name of the process
    name: String,
    /// Data needed for the runtime
    data: Arc<Data>,
    /// The tempfile that output from the wasmtime process writes to
    output: Arc<NamedTempFile>,
    /// A channel to send status updates on the runtime
    status_sender: Sender<Status>,
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
    pub async fn new<L: AsRef<Path> + Send + Sync + 'static>(
        name: String,
        module_data: Vec<u8>,
        env: HashMap<String, String>,
        args: Vec<String>,
        dirs: HashMap<PathBuf, Option<PathBuf>>,
        log_dir: L,
        status_sender: Sender<Status>,
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

        let (interrupt_handle, handle) = self.spawn_wasmtime(output_write).await?;

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
    // channel. Due to the Instance type not being Send safe, all of the logic
    // needs to be done within the spawned task
    async fn spawn_wasmtime(
        &self,
        output_write: std::fs::File,
    ) -> anyhow::Result<(InterruptHandle, JoinHandle<anyhow::Result<()>>)> {
        // Clone the module data Arc so it can be moved
        let data = self.data.clone();
        let status_sender = self.status_sender.clone();
        let (tx, rx) = oneshot::channel();

        let name = self.name.clone();
        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let env: Vec<(String, String)> = data
                .env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let stdout = unsafe { cap_std::fs::File::from_std(output_write.try_clone()?) };
            let stdout = wasi_cap_std_sync::file::File::from_cap_std(stdout);
            let stderr = unsafe { cap_std::fs::File::from_std(output_write.try_clone()?) };
            let stderr = wasi_cap_std_sync::file::File::from_cap_std(stderr);

            // Build the WASI instance and then generate a list of WASI modules
            let ctx_builder_snapshot = WasiCtxBuilder::new();
            let mut ctx_builder_snapshot = ctx_builder_snapshot
                .args(&data.args)?
                .envs(&env)?
                .stdout(Box::new(stdout))
                .stderr(Box::new(stderr));

            let stdout = unsafe { cap_std::fs::File::from_std(output_write.try_clone()?) };
            let stdout = wasi_cap_std_sync::file::File::from_cap_std(stdout);
            let stderr = unsafe { cap_std::fs::File::from_std(output_write.try_clone()?) };
            let stderr = wasi_cap_std_sync::file::File::from_cap_std(stderr);

            let ctx_builder_unstable = WasiCtxBuilder::new();
            let mut ctx_builder_unstable = ctx_builder_unstable
                .args(&data.args)?
                .envs(&env)?
                .stdout(Box::new(stdout))
                .stderr(Box::new(stderr));

            for (key, value) in data.dirs.iter() {
                let guest_dir = value.as_ref().unwrap_or(key);
                debug!(
                    "{} mounting hostpath {} as guestpath {}",
                    &name,
                    key.display(),
                    guest_dir.display()
                );
                let preopen_dir = unsafe { cap_std::fs::Dir::open_ambient_dir(key) }?;
                ctx_builder_snapshot =
                    ctx_builder_snapshot.preopened_dir(preopen_dir, guest_dir)?;
                let preopen_dir = unsafe { cap_std::fs::Dir::open_ambient_dir(key) }?;
                ctx_builder_unstable =
                    ctx_builder_unstable.preopened_dir(preopen_dir, guest_dir)?;
            }
            let wasi_ctx_snapshot = ctx_builder_snapshot.build()?;
            let wasi_ctx_unstable = ctx_builder_unstable.build()?;
            let mut config = wasmtime::Config::new();
            config.interruptable(true);
            let engine = wasmtime::Engine::new(&config)?;
            let store = wasmtime::Store::new(&engine);
            let interrupt = store.interrupt_handle()?;
            tx.send(interrupt)
                .map_err(|_| anyhow::anyhow!("Unable to send interrupt back to main thread"))?;

            let wasi_snapshot = Wasi::new(
                &store,
                std::rc::Rc::new(std::cell::RefCell::new(wasi_ctx_snapshot)),
            );
            let wasi_unstable = WasiUnstable::new(
                &store,
                std::rc::Rc::new(std::cell::RefCell::new(wasi_ctx_unstable)),
            );
            let module = match wasmtime::Module::new(&engine, &data.module_data) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to create module";
                    error!("{} {}: {:?}", &name, message, e);
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
            // Iterate through the module includes and resolve imports
            let imports = module
                .imports()
                .map(|i| {
                    let name = i.name().unwrap();
                    // This is super funky logic, but it matches what is in 0.12.0
                    let export = match i.module() {
                        "wasi_snapshot_preview1" => wasi_snapshot.get_export(name),
                        "wasi_unstable" => wasi_unstable.get_export(name),
                        other => bail!("import module `{}` was not found", other),
                    };
                    match export {
                        Some(export) => Ok(export.clone().into()),
                        None => bail!("import `{}` was not found in module `{}`", name, i.module()),
                    }
                })
                .collect::<Result<Vec<_>, _>>();
            let imports = match imports {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to load module";
                    error!("{} {}: {:?}", &name, message, e);
                    send(
                        &status_sender,
                        &name,
                        Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        },
                    );

                    return Err(e);
                }
            };

            let instance = match wasmtime::Instance::new(&store, &module, &imports) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to instantiate module";
                    error!("{} {}: {:?}", &name, message, e);
                    send(
                        &status_sender,
                        &name,
                        Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        },
                    );

                    // Converting from anyhow
                    return Err(anyhow::anyhow!("{}: {}", message, e));
                }
            };

            // NOTE(taylor): In the future, if we want to pass args directly, we'll
            // need to do a bit more to pass them in here.
            info!("{} starting run of module", &name);
            send(
                &status_sender,
                &name,
                Status::Running {
                    timestamp: chrono::Utc::now(),
                },
            );

            let export = instance
                .get_export("_start")
                .ok_or_else(|| anyhow::anyhow!("_start import doesn't exist in wasm module"))?;
            let func = match export {
                wasmtime::Extern::Func(f) => f,
                _ => {
                    let message = "_start import was not a function. This is likely a problem with the module";
                    error!("{} {}", &name, message);
                    send(
                        &status_sender,
                        &name,
                        Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        },
                    );

                    return Err(anyhow::anyhow!(message));
                }
            };
            match func.call(&[]) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(_) => {}
                Err(e) => {
                    let message = "unable to run module";
                    error!("{} {}: {:?}", &name, message, e);
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

            info!("{} module run complete", &name);
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
        let interrupt = rx.await?;
        Ok((interrupt, handle))
    }
}

fn send(sender: &Sender<Status>, name: &str, status: Status) {
    match sender.blocking_send(status) {
        Err(e) => warn!("{} error sending wasi status: {:?}", name, e),
        Ok(_) => debug!("{} send completed.", name),
    }
}
