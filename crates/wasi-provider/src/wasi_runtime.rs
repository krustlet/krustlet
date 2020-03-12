use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::Path;

use failure::{bail, format_err};
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateRunning, ContainerStateTerminated, ContainerStateWaiting,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use log::{error, info};
use tempfile::NamedTempFile;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::watch::{self, Receiver};
use tokio::task::JoinHandle;
use wasi_common::preopen_dir;
use wasmtime_wasi::old::snapshot_0::Wasi as WasiUnstable;
use wasmtime_wasi::{Wasi, WasiCtxBuilder};

/// WasiRuntime provides a WASI compatible runtime. A runtime should be used for
/// each "instance" of a process and can be passed to a thread pool for running
pub struct WasiRuntime {
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

    /// The tempfile that output from the wasmtime process writes to
    output: NamedTempFile,
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
    pub fn new<M: AsRef<Path>, L: AsRef<Path>>(
        module_path: M,
        env: HashMap<String, String>,
        args: Vec<String>,
        dirs: HashMap<String, Option<String>>,
        log_dir: L,
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
            output: NamedTempFile::new_in(log_dir)?,
        })
    }

    pub async fn run(&self) -> Result<RuntimeHandle<tokio::fs::File>, failure::Error> {
        let output = self.output.reopen()?;
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
            ctx_builder_snapshot = ctx_builder_snapshot.preopened_dir(preopen_dir(key)?, guest_dir);
            ctx_builder_unstable = ctx_builder_unstable.preopened_dir(preopen_dir(key)?, guest_dir);
        }
        let wasi_ctx_snapshot = ctx_builder_snapshot.build()?;
        let wasi_ctx_unstable = ctx_builder_unstable.build()?;

        // Clone the module data so it can be moved and we don't have to worry
        // about the lifetime of the struct data
        let module_data = self.module_data.clone();
        // We could get multiple status updates, so this gives a little
        // breathing room while avoiding blocking. This is currently super
        // naive, but will work for now. In the future, we may just want to use
        // a try_send with a retry
        let (status_sender, status_recv) = watch::channel(ContainerState {
            waiting: Some(ContainerStateWaiting {
                message: Some("No status has been received from the process".into()),
                reason: None,
            }),
            ..Default::default()
        });
        let handle = tokio::task::spawn_blocking(move || -> Result<_, failure::Error> {
            let engine = wasmtime::Engine::default();
            let store = wasmtime::Store::new(&engine);
            let wasi_snapshot = Wasi::new(&store, wasi_ctx_snapshot);
            let wasi_unstable = WasiUnstable::new(&store, wasi_ctx_unstable);
            let module = match wasmtime::Module::new(&store, &module_data) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => Ok(m),
                Err(e) => {
                    let message = "unable to create module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(ContainerState {
                            terminated: Some(ContainerStateTerminated {
                                message: Some(message.into()),
                                reason: None,
                                exit_code: 1,
                                finished_at: Some(Time(chrono::Utc::now())),
                                ..Default::default()
                            }),
                            ..Default::default()
                        })
                        .expect("status should be able to send");
                    // Converting from anyhow
                    Err(format_err!("{}: {}", message, e))
                }
            }?;
            // Iterate through the module includes and resolve imports
            let imports = match module
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
                .collect::<Result<Vec<_>, _>>()
            {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => Ok(m),
                Err(e) => {
                    let message = "unable to load module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(ContainerState {
                            terminated: Some(ContainerStateTerminated {
                                message: Some(message.into()),
                                reason: None,
                                exit_code: 1,
                                finished_at: Some(Time(chrono::Utc::now())),
                                ..Default::default()
                            }),
                            ..Default::default()
                        })
                        .expect("status should be able to send");
                    Err(e)
                }
            }?;

            let instance = match wasmtime::Instance::new(&module, &imports) {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => Ok(m),
                Err(e) => {
                    let message = "unable to instantiate module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(ContainerState {
                            terminated: Some(ContainerStateTerminated {
                                message: Some(message.into()),
                                reason: None,
                                exit_code: 1,
                                finished_at: Some(Time(chrono::Utc::now())),
                                ..Default::default()
                            }),
                            ..Default::default()
                        })
                        .expect("status should be able to send");
                    // Converting from anyhow
                    Err(format_err!("{}: {}", message, e))
                }
            }?;

            // NOTE(taylor): In the future, if we want to pass args directly, we'll
            // need to do a bit more to pass them in here.
            info!("starting run of module");
            status_sender
                .broadcast(ContainerState {
                    running: Some(ContainerStateRunning {
                        started_at: Some(Time(chrono::Utc::now())),
                    }),
                    ..Default::default()
                })
                .expect("status should be able to send");
            match instance
                .get_export("_start")
                .expect("export")
                .func()
                .unwrap()
                .call(&[])
            {
                // We can't map errors here or it moves the send channel, so we
                // do it in a match
                Ok(m) => Ok(m),
                Err(e) => {
                    let message = "unable to run module";
                    error!("{}: {:?}", message, e);
                    status_sender
                        .broadcast(ContainerState {
                            terminated: Some(ContainerStateTerminated {
                                message: Some(message.into()),
                                reason: None,
                                exit_code: 1,
                                finished_at: Some(Time(chrono::Utc::now())),
                                ..Default::default()
                            }),
                            ..Default::default()
                        })
                        .expect("status should be able to send");
                    // Converting from anyhow
                    Err(format_err!("{}: {}", message, e))
                }
            }?;

            info!("module run complete");
            status_sender
                .broadcast(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        message: Some("Module run completed".into()),
                        exit_code: 0,
                        finished_at: Some(Time(chrono::Utc::now())),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .expect("status should be able to send");
            Ok(())
        });

        Ok(RuntimeHandle::new(
            tokio::fs::File::from_std(self.output.reopen()?),
            handle,
            status_recv,
        ))
    }
}

/// Represents a handle to a running WASI instance. Right now, this is
/// experimental and just for use with the [crate::WasiProvider]. If we like
/// this pattern, we will expose it as part of the kubelet crate
pub struct RuntimeHandle<R: AsyncReadExt + AsyncSeekExt + Unpin> {
    output: BufReader<R>,
    handle: JoinHandle<Result<(), failure::Error>>,
    status_channel: Receiver<ContainerState>,
}

impl<R: AsyncReadExt + AsyncSeekExt + Unpin> RuntimeHandle<R> {
    /// Create a new handle with the given reader for log output and a handle to
    /// the running tokio task. The sender part of the channel should be given
    /// to the running process and the receiver half passed to this constructor
    /// to be used for reporting current status
    pub fn new(
        output: R,
        handle: JoinHandle<Result<(), failure::Error>>,
        status_channel: Receiver<ContainerState>,
    ) -> Self {
        RuntimeHandle {
            output: BufReader::new(output),
            handle,
            status_channel,
        }
    }

    pub async fn output(&mut self) -> Result<Vec<u8>, failure::Error> {
        let mut output = Vec::new();
        self.output.read_to_end(&mut output).await?;
        // Reset the seek location for the next call to read from the file
        // NOTE: This is a little janky, but the Tokio BufReader does not
        // implement the AsyncSeek trait
        self.output.get_mut().seek(SeekFrom::Start(0)).await?;
        Ok(output)
    }

    pub async fn stop(&mut self) -> Result<(), failure::Error> {
        // TODO: Send an actual stop signal once there is support in wasmtime
        self.wait().await?;
        unimplemented!("There is currently no way to stop a running wasmtime instance")
    }

    pub async fn status(&self) -> Result<ContainerState, failure::Error> {
        // NOTE: For those who modify this in the future, borrow must be as
        // short lived as possible. We do not use the recv method because it
        // uses the value each time and blocks on the next call, whereas we want
        // to return the last sent value until updated
        Ok((*self.status_channel.borrow()).clone())
    }

    // For now this is private (for use in testing and in stop). If we find a
    // need to expose it, we can do that later
    async fn wait(&mut self) -> Result<(), failure::Error> {
        (&mut self.handle).await.unwrap()
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
        .expect("wasi runtime init");
        let mut handle = wr.run().await.expect("runtime handle");
        handle.wait().await.expect("successful run");

        let output = handle.output().await.unwrap();
        assert_eq!("Hello, world!\n".to_string().into_bytes(), output);

        let status = handle.status().await.unwrap();
        assert!(status.terminated.is_some());

        // TODO: Once we add args support and other things that could actually
        // cause a failure on start, we can test the intermediate state. Same
        // thing when we get a longer running module we can test for running
        // state
    }
}
