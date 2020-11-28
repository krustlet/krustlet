use crate::wasi_runtime::{HandleFactory, Runtime};
use kubelet::container::{Container, Handle};
use kubelet::state::ResourceState;

pub(crate) mod running;
pub(crate) mod terminated;
pub(crate) mod waiting;

pub(crate) type ContainerHandle = Handle<Runtime, HandleFactory>;

struct ContainerState;

impl ResourceState for ContainerState {
    type Manifest = Container;
}
