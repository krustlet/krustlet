use std::error;
use std::fmt;

use wasm3::Module;

type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeError {
    kind: RuntimeErrorKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeErrorKind {
    CouldNotStart,
    CouldNotStop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeStatus {
    Running,
    Stopped,
}

impl RuntimeError {
    fn __description(&self) -> &str {
        match self.kind {
            RuntimeErrorKind::CouldNotStart => "cannot start runtime",
            RuntimeErrorKind::CouldNotStop => "cannot stop runtime",
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, self.msg)
    }
}

/// A runtime context for running wasm modules with wasm3
struct Runtime {
    runtime: wasm3::Runtime,
}

impl Runtime {
    fn new(module: Vec<u8>, stack_size: u32) -> Result<Runtime> {
        let env = Environment::new()?;
        let rt = env.create_runtime(stack_size)?;
    }

    fn start(&mut self) -> Result<()> {
    }

    fn stop(&mut self) -> Result<()> {
        Error(RuntimeError{ kind: RuntimeErrorKind::CouldNotStop })
    }

    fn status(&self) -> Result<RuntimeStatus> {
        RuntimeStatus::Running
    }
}
