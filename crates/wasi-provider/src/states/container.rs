use async_trait::async_trait;
use kubelet::container::{Container, Status};
use kubelet::state::ResourceState;
pub(crate) mod running;
pub(crate) mod terminated;
pub(crate) mod waiting;

struct SharedContainerState;

struct ContainerState;

#[async_trait]
impl ResourceState for ContainerState {
    type Manifest = Container;
    type Status = Status;
    type SharedState = SharedContainerState;
    async fn async_drop(self, _shared_state: &mut Self::SharedState) {}
}
