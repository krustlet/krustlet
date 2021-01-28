// Test that State<T> can only transition to State<T>
// edition:2018
extern crate async_trait;
extern crate kubelet;
extern crate krator;

use krator::{Transition, state::StateHolder, ObjectState};
use kubelet::pod::{Pod, Status, state::Stub};

struct PodState;
struct ProviderState;

#[async_trait::async_trait]
impl ObjectState for PodState {
    type Manifest = Pod;
    type Status = Status;
    type SharedState = ProviderState;
    async fn async_drop(self, _provider_state: &mut ProviderState) { }
}

fn main() {
    // This fails because `state` is a private field. Use Transition::next classmethod instead.
    let _transition = Transition::<PodState>::Next(StateHolder {
        state: Box::new(Stub),
    });
}
