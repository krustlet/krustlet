use anyhow::bail;
use log::{debug, error, info};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::NamedTempFile;
use tokio::sync::{
    oneshot,
    watch::{self, Sender},
};
use tokio::task::JoinHandle;
use wasi_common::preopen_dir;
use wasmtime::InterruptHandle;
use wasmtime_wasi::old::snapshot_0::Wasi as WasiUnstable;
use wasmtime_wasi::{Wasi, WasiCtxBuilder};

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
    /// Data needed for the runtime
    data: Arc<Data>,
    /// The tempfile that output from the wasmtime process writes to
    output: Arc<NamedTempFile>,
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
        module_data: Vec<u8>,
        env: HashMap<String, String>,
        args: Vec<String>,
        dirs: HashMap<PathBuf, Option<PathBuf>>,
        log_dir: L,
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
            data: Arc::new(Data {
                module_data,
                env,
                args,
                dirs,
            }),
            output: Arc::new(temp),
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

        let (status_sender, status_recv) = watch::channel(Status::Waiting {
            timestamp: chrono::Utc::now(),
            message: "No status has been received from the process".into(),
        });
        let (interrupt_handle, handle) = self.spawn_wasmtime(status_sender, output_write).await?;

        let log_handle_factory = HandleFactory {
            temp: self.output.clone(),
        };

        Ok(ContainerHandle::new(
            Runtime {
                handle,
                interrupt_handle,
            },
            log_handle_factory,
            status_recv,
        ))
    }

    // Spawns a running wasmtime instance with the given context and status
    // channel. Due to the Instance type not being Send safe, all of the logic
    // needs to be done within the spawned task
    async fn spawn_wasmtime(
        &self,
        status_sender: Sender<Status>,
        output_write: std::fs::File,
    ) -> anyhow::Result<(InterruptHandle, JoinHandle<anyhow::Result<()>>)> {
        // Clone the module data Arc so it can be moved
        let data = self.data.clone();

        let (tx, rx) = oneshot::channel();

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            // Build the WASI instance and then generate a list of WASI modules
            let mut ctx_builder_snapshot = WasiCtxBuilder::new();
            let mut ctx_builder_snapshot = ctx_builder_snapshot
                .args(&data.args)
                .envs(&data.env)
                .stdout(wasi_common::OsFile::try_from(output_write.try_clone()?)?)
                .stderr(wasi_common::OsFile::try_from(output_write.try_clone()?)?);
            let mut ctx_builder_unstable = wasi_common::old::snapshot_0::WasiCtxBuilder::new();
            let mut ctx_builder_unstable = ctx_builder_unstable
                .args(&data.args)
                .envs(&data.env)
                .stdout(output_write.try_clone()?)
                .stderr(output_write);

            for (key, value) in data.dirs.iter() {
                let guest_dir = value.as_ref().unwrap_or(key);
                debug!(
                    "mounting hostpath {} as guestpath {}",
                    key.display(),
                    guest_dir.display()
                );
                ctx_builder_snapshot =
                    ctx_builder_snapshot.preopened_dir(preopen_dir(key)?, guest_dir);
                ctx_builder_unstable =
                    ctx_builder_unstable.preopened_dir(preopen_dir(key)?, guest_dir);
            }
            let wasi_ctx_snapshot = ctx_builder_snapshot.build()?;
            let wasi_ctx_unstable = ctx_builder_unstable.build()?;
            let mut config = wasmtime::Config::new();
            config.interruptable(true);
            let engine = wasmtime::Engine::new(&config);
            let store = wasmtime::Store::new(&engine);
            let interrupt = store.interrupt_handle()?;
            tx.send(interrupt)
                .map_err(|_| anyhow::anyhow!("Unable to send interrupt back to main thread"))?;

            let wasi_snapshot = Wasi::new(&store, wasi_ctx_snapshot);
            let wasi_unstable = WasiUnstable::new(&store, wasi_ctx_unstable);
            let module = match wasmtime::Module::new(&engine, &data.module_data) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to create module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        })
                        .expect("status should be able to send");
                    return Err(anyhow::anyhow!("{}: {}", message, e));
                }
            };
            // Iterate through the module includes and resolve imports
            let imports = module
                .imports()
                .map(|i| {
                    // This is super funky logic, but it matches what is in 0.12.0
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
                .collect::<Result<Vec<_>, _>>();
            let imports = match imports {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to load module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        })
                        .expect("status should be able to send");
                    return Err(e);
                }
            };

            let instance = match wasmtime::Instance::new(&store, &module, &imports) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to instantiate module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        })
                        .expect("status should be able to send");
                    // Converting from anyhow
                    return Err(anyhow::anyhow!("{}: {}", message, e));
                }
            };

            // NOTE(taylor): In the future, if we want to pass args directly, we'll
            // need to do a bit more to pass them in here.
            info!("starting run of module");
            status_sender
                .broadcast(Status::Running {
                    timestamp: chrono::Utc::now(),
                })
                .expect("status should be able to send");
            let export = instance
                .get_export("_start")
                .ok_or_else(|| anyhow::anyhow!("_start import doesn't exist in wasm module"))?;
            let func = match export {
                wasmtime::Extern::Func(f) => f,
                _ => {
                    return Err(anyhow::anyhow!(
                    "_start import was not a function. This is likely a problem with the module"
                ))
                }
            };
            match func.call(&[]) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(_) => {}
                Err(e) => {
                    let message = "unable to run module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(Status::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        })
                        .expect("status should be able to send");
                    return Err(anyhow::anyhow!("{}: {}", message, e));
                }
            };

            info!("module run complete");
            status_sender
                .broadcast(Status::Terminated {
                    failed: false,
                    message: "Module run completed".into(),
                    timestamp: chrono::Utc::now(),
                })
                .expect("status should be able to send");
            Ok(())
        });
        // Wait for the interrupt to be sent back to us
        let interrupt = rx.await?;
        Ok((interrupt, handle))
    }
}
