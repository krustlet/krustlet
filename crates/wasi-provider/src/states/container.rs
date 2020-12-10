use kubelet::container::{Container, Status};
use kubelet::state::ResourceState;

pub(crate) mod running;
pub(crate) mod terminated;
pub(crate) mod waiting;

struct SharedContainerState;

struct ContainerState;

impl ResourceState for ContainerState {
    type Manifest = Container;
    type Status = Status;
}
