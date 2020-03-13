use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::bail;
use log::{error, info};
use tempfile::NamedTempFile;
use tokio::sync::watch::{self, Sender};
use tokio::task::JoinHandle;
use wasi_common::preopen_dir;
use wasmtime_wasi::old::snapshot_0::Wasi as WasiUnstable;
use wasmtime_wasi::{Wasi, WasiCtxBuilder};

use crate::handle::RuntimeHandle;
use kubelet::ContainerStatus;

/// WasiRuntime provides a WASI compatible runtime. A runtime should be used for
/// each "instance" of a process and can be passed to a thread pool for running
pub struct WasiRuntime {
    /// binary module data to be run as a wasm module
    module_data: Arc<Vec<u8>>,
    /// key/value environment variables made available to the wasm process
    env: HashMap<String, String>,
    /// the arguments passed as the command-line arguments list
    args: Vec<String>,
    /// a hash map of local file system paths to optional path names in the runtime
    /// (e.g. /tmp/foo/myfile -> /app/config). If the optional value is not given,
    /// the same path will be allowed in the runtime
    dirs: HashMap<String, Option<String>>,

    /// The tempfile that output from the wasmtime process writes to
    output: Arc<NamedTempFile>,
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
    pub async fn new<
        M: AsRef<Path> + Send + Sync + 'static,
        L: AsRef<Path> + Send + Sync + 'static,
    >(
        module_path: M,
        env: HashMap<String, String>,
        args: Vec<String>,
        dirs: HashMap<String, Option<String>>,
        log_dir: L,
    ) -> anyhow::Result<Self> {
        let (module_data, temp) =
            tokio::task::spawn_blocking(move || -> anyhow::Result<(Vec<u8>, NamedTempFile)> {
                Ok((
                    wat::parse_file(module_path)?,
                    NamedTempFile::new_in(log_dir)?,
                ))
            })
            .await??;

        // We need to use named temp file because we need multiple file handles
        // and if we are running in the temp dir, we run the possibility of the
        // temp file getting cleaned out from underneath us while running. If we
        // think it necessary, we can make these permanent files with a cleanup
        // loop that runs elsewhere. These will get deleted when the reference
        // is dropped
        Ok(WasiRuntime {
            module_data: Arc::new(module_data),
            env,
            args,
            dirs,
            output: Arc::new(temp),
        })
    }

    pub async fn start(&self) -> anyhow::Result<RuntimeHandle<tokio::fs::File>> {
        let temp = self.output.clone();
        // Because a reopen is blocking, run in a blocking task to get new
        // handles to the tempfile
        let (output_write, output_read) = tokio::task::spawn_blocking(
            move || -> anyhow::Result<(std::fs::File, std::fs::File)> {
                Ok((temp.reopen()?, temp.reopen()?))
            },
        )
        .await??;

        // Build the WASI instance and then generate a list of WASI modules
        let mut ctx_builder_snapshot = WasiCtxBuilder::new();
        // For some reason if I didn't split these out, the compiler got mad
        let mut ctx_builder_snapshot = ctx_builder_snapshot
            .args(&self.args)
            .envs(&self.env)
            .stdout(output_write.try_clone()?)
            .stderr(output_write.try_clone()?);
        let mut ctx_builder_unstable = wasi_common::old::snapshot_0::WasiCtxBuilder::new()
            .args(&self.args)
            .envs(&self.env)
            .stdout(output_write.try_clone()?)
            .stderr(output_write);

        for (key, value) in self.dirs.iter() {
            let guest_dir = value.as_ref().unwrap_or(key);
            ctx_builder_snapshot = ctx_builder_snapshot.preopened_dir(preopen_dir(key)?, guest_dir);
            ctx_builder_unstable = ctx_builder_unstable.preopened_dir(preopen_dir(key)?, guest_dir);
        }
        let wasi_ctx_snapshot = ctx_builder_snapshot.build()?;
        let wasi_ctx_unstable = ctx_builder_unstable.build()?;

        let (status_sender, status_recv) = watch::channel(ContainerStatus::Waiting {
            timestamp: chrono::Utc::now(),
            message: "No status has been received from the process".into(),
        });
        let handle = self.spawn_wasmtime(status_sender, wasi_ctx_snapshot, wasi_ctx_unstable);

        Ok(RuntimeHandle::new(
            tokio::fs::File::from_std(output_read),
            handle,
            status_recv,
        ))
    }

    // Spawns a running wasmtime instance with the given context and status
    // channel. Due to the Instance type not being Send safe, all of the logic
    // needs to be done within the spawned task
    fn spawn_wasmtime(
        &self,
        status_sender: Sender<ContainerStatus>,
        wasi_ctx_snapshot: wasi_common::WasiCtx,
        wasi_ctx_unstable: wasi_common::old::snapshot_0::WasiCtx,
    ) -> JoinHandle<anyhow::Result<()>> {
        // Clone the module data Arc so it can be moved
        let module_data = self.module_data.clone();

        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let engine = wasmtime::Engine::default();
            let store = wasmtime::Store::new(&engine);
            let wasi_snapshot = Wasi::new(&store, wasi_ctx_snapshot);
            let wasi_unstable = WasiUnstable::new(&store, wasi_ctx_unstable);
            let module = match wasmtime::Module::new(&store, module_data.as_ref()) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to create module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(ContainerStatus::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        })
                        .expect("status should be able to send");
                    // Converting from anyhow
                    return Err(anyhow::anyhow!("{}: {}", message, e));
                }
            };
            // Iterate through the module includes and resolve imports
            let imports = module
                .imports()
                .iter()
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
                        .broadcast(ContainerStatus::Terminated {
                            failed: true,
                            message: message.into(),
                            timestamp: chrono::Utc::now(),
                        })
                        .expect("status should be able to send");
                    return Err(e);
                }
            };

            let instance = match wasmtime::Instance::new(&module, &imports) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => m,
                Err(e) => {
                    let message = "unable to instantiate module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(ContainerStatus::Terminated {
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
                .broadcast(ContainerStatus::Running {
                    timestamp: chrono::Utc::now(),
                })
                .expect("status should be able to send");
            match instance
                .get_export("_start")
                .expect("_start import should exist in wasm module")
                .func()
                .unwrap()
                .call(&[])
            {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(_) => {}
                Err(e) => {
                    let message = "unable to run module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(ContainerStatus::Terminated {
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
                .broadcast(ContainerStatus::Terminated {
                    failed: false,
                    message: "Module run completed".into(),
                    timestamp: chrono::Utc::now(),
                })
                .expect("status should be able to send");
            Ok(())
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_run() {
        let wr = WasiRuntime::new(
            "./testdata/hello-world.wasm",
            HashMap::default(),
            Vec::default(),
            HashMap::default(),
            std::env::current_dir().unwrap(),
        )
        .await
        .expect("wasi runtime init");
        let mut handle = wr.start().await.expect("runtime handle");
        handle.wait().await.expect("successful run");

        let mut output = Vec::new();
        handle.output(&mut output).await.unwrap();
        assert_eq!("Hello, world!\n".to_string().into_bytes(), output);

        let status = handle.status().await.unwrap();
        assert!(match status {
            ContainerStatus::Terminated { .. } => true,
            _ => false,
        });

        // TODO: Once we add args support and other things that could actually
        // cause a failure on start, we can test the intermediate state. Same
        // thing when we get a longer running module we can test for running
        // state
    }
}
