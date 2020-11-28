use kubelet::container::Container;
use kubelet::state::ResourceState;

pub(crate) mod running;
pub(crate) mod terminated;
pub(crate) mod waiting;

struct ContainerState;

impl ResourceState for ContainerState {
    type Manifest = Container;
}
