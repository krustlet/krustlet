//! `container` is a collection of utilities surrounding the Kubernetes container API.
mod handle;
mod status;

pub use handle::Handle;
pub use status::Status;
