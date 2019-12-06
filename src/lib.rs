#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate failure;

pub mod kubelet;
pub mod node;
pub mod pod;
pub mod server;
pub mod wasm;
