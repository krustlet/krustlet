use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use failure::{bail, format_err};
use log::info;
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

        Ok(WasiRuntime {
            module_data,
            env,
            args,
            dirs,
        })
    }

    pub fn run(&self, output: File) -> Result<(), failure::Error> {
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
    use tempfile::NamedTempFile;
    use tokio::io::AsyncReadExt;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn test_run() {
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

        let mut output = BufReader::new(tokio::fs::File::from_std(output.into_file()));
        output.read_to_string(&mut stdout).await.unwrap();
        assert_eq!("Hello, world!\n", stdout);
    }
}
