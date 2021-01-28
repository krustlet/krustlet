use k8s_openapi::Metadata;
use krator::{
    Manifest, ObjectState, ObjectStatus, Operator, OperatorRuntime, State, Transition, TransitionTo,
};
use kube::api::ListParams;
use kube_derive::CustomResource;
use log::info;
use rand::seq::IteratorRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(CustomResource, Debug, Serialize, Deserialize, Clone, Default)]
#[kube(
    group = "animals.com",
    version = "v1",
    kind = "Moose",
    derive = "Default",
    status = "MooseStatus",
    namespaced
)]
struct MooseSpec {
    height: f64,
    weight: f64,
    antlers: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum MoosePhase {
    Asleep,
    Hungry,
    Roaming,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MooseStatus {
    phase: Option<MoosePhase>,
    message: Option<String>,
}

impl ObjectStatus for MooseStatus {
    fn failed(e: &str) -> MooseStatus {
        MooseStatus {
            message: Some(format!("Error tracking moose: {}.", e)),
            phase: None,
        }
    }

    fn json_patch(&self) -> serde_json::Value {
        // Generate a map containing only set fields.
        let mut status = serde_json::Map::new();

        if let Some(phase) = self.phase.clone() {
            status.insert("phase".to_string(), serde_json::json!(phase));
        };

        if let Some(message) = self.message.clone() {
            status.insert("message".to_string(), serde_json::Value::String(message));
        };

        // Create status patch with map.
        serde_json::json!({ "status": serde_json::Value::Object(status) })
    }
}

struct MooseState {
    name: String,
    food: f64,
}

#[async_trait::async_trait]
impl ObjectState for MooseState {
    type Manifest = Moose;
    type Status = MooseStatus;
    type SharedState = SharedMooseState;
    async fn async_drop(self, shared: &mut Self::SharedState) {
        shared.friends.remove(&self.name);
    }
}

#[derive(Debug, Default)]
/// Moose was tagged for tracking.
struct Tagged;

#[async_trait::async_trait]
impl State<MooseState> for Tagged {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedMooseState>>,
        state: &mut MooseState,
        _manifest: Manifest<Moose>,
    ) -> Transition<MooseState> {
        info!("Found new moose named {}!", state.name);
        shared
            .write()
            .await
            .friends
            .insert(state.name.clone(), HashSet::new());
        Transition::next(self, Roam)
    }

    async fn status(
        &self,
        _state: &mut MooseState,
        _manifest: &Moose,
    ) -> anyhow::Result<MooseStatus> {
        Ok(MooseStatus {
            phase: Some(MoosePhase::Roaming),
            message: None,
        })
    }
}

// Explicitly implement TransitionTo
impl TransitionTo<Roam> for Tagged {}

// Derive TransitionTo
#[derive(Debug, Default, TransitionTo)]
// Specify valid next states.
#[transition_to(Eat)]
/// Moose is roaming the wilderness.
struct Roam;

async fn make_friend(name: &str, shared: &Arc<RwLock<SharedMooseState>>) -> Option<String> {
    let mut mooses = shared.write().await;
    let mut rng = rand::thread_rng();
    let other_meese = mooses
        .friends
        .keys()
        .map(|s| s.to_owned())
        .choose_multiple(&mut rng, mooses.friends.len());
    for other_moose in other_meese {
        if name == other_moose {
            continue;
        }
        let friends = mooses.friends.get_mut(&other_moose).unwrap();
        if !friends.contains(name) {
            friends.insert(name.to_string());
            return Some(other_moose.to_string());
        }
    }
    return None;
}

#[async_trait::async_trait]
impl State<MooseState> for Roam {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedMooseState>>,
        state: &mut MooseState,
        _manifest: Manifest<Moose>,
    ) -> Transition<MooseState> {
        loop {
            tokio::time::delay_for(std::time::Duration::from_secs(2)).await;
            state.food -= 5.0;
            if state.food <= 10.0 {
                return Transition::next(self, Eat);
            }
            let r: f64 = {
                let mut rng = rand::thread_rng();
                rng.gen()
            };
            if r < 0.05 {
                if let Some(other_moose) = make_friend(&state.name, &shared).await {
                    info!("{} made friends with {}!", state.name, other_moose);
                }
            }
        }
    }

    async fn status(
        &self,
        _state: &mut MooseState,
        _manifest: &Moose,
    ) -> anyhow::Result<MooseStatus> {
        Ok(MooseStatus {
            phase: Some(MoosePhase::Roaming),
            message: Some("Gahrooo!".to_string()),
        })
    }
}

#[derive(Debug, Default, TransitionTo)]
#[transition_to(Sleep)]
/// Moose is eating.
struct Eat;

#[async_trait::async_trait]
impl State<MooseState> for Eat {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedMooseState>>,
        state: &mut MooseState,
        manifest: Manifest<Moose>,
    ) -> Transition<MooseState> {
        let moose = manifest.latest();
        state.food = moose.spec.weight / 10.0;
        tokio::time::delay_for(std::time::Duration::from_secs((state.food / 10.0) as u64)).await;
        Transition::next(self, Sleep)
    }

    async fn status(
        &self,
        _state: &mut MooseState,
        _manifest: &Moose,
    ) -> anyhow::Result<MooseStatus> {
        Ok(MooseStatus {
            phase: Some(MoosePhase::Hungry),
            message: Some("*munch*".to_string()),
        })
    }
}

#[derive(Debug, Default, TransitionTo)]
#[transition_to(Roam)]
/// Moose is sleeping.
struct Sleep;

#[async_trait::async_trait]
impl State<MooseState> for Sleep {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedMooseState>>,
        _state: &mut MooseState,
        _manifest: Manifest<Moose>,
    ) -> Transition<MooseState> {
        tokio::time::delay_for(std::time::Duration::from_secs(20)).await;
        Transition::next(self, Roam)
    }

    async fn status(
        &self,
        _state: &mut MooseState,
        _manifest: &Moose,
    ) -> anyhow::Result<MooseStatus> {
        Ok(MooseStatus {
            phase: Some(MoosePhase::Asleep),
            message: Some("zzzzzz".to_string()),
        })
    }
}

#[derive(Debug, Default)]
/// Moose was released from our care.
struct Released;

#[async_trait::async_trait]
impl State<MooseState> for Released {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedMooseState>>,
        _state: &mut MooseState,
        _manifest: Manifest<Moose>,
    ) -> Transition<MooseState> {
        info!("Moose tagged for release!");
        Transition::Complete(Ok(()))
    }

    async fn status(
        &self,
        state: &mut MooseState,
        _manifest: &Moose,
    ) -> anyhow::Result<MooseStatus> {
        Ok(MooseStatus {
            phase: None,
            message: Some(format!("Bye, {}!", state.name)),
        })
    }
}

struct SharedMooseState {
    friends: HashMap<String, HashSet<String>>,
}

struct MooseTracker {
    shared: Arc<RwLock<SharedMooseState>>,
}

impl MooseTracker {
    fn new() -> Self {
        let shared = Arc::new(RwLock::new(SharedMooseState {
            friends: HashMap::new(),
        }));
        MooseTracker { shared }
    }
}

#[async_trait::async_trait]
impl Operator for MooseTracker {
    type Manifest = Moose;
    type Status = MooseStatus;
    type InitialState = Tagged;
    type DeletedState = Released;
    type ObjectState = MooseState;

    async fn initialize_object_state(
        &self,
        manifest: &Self::Manifest,
    ) -> anyhow::Result<Self::ObjectState> {
        let name = manifest.metadata().name.clone().unwrap();
        Ok(MooseState {
            name,
            food: manifest.spec.weight / 10.0,
        })
    }

    async fn shared_state(&self) -> Arc<RwLock<SharedMooseState>> {
        Arc::clone(&self.shared)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let kubeconfig = kube::Config::infer().await?;
    let tracker = MooseTracker::new();

    // Only track mooses in Glacier NP
    let params = ListParams::default().labels("nps.gov/park=glacier");

    let mut runtime = OperatorRuntime::new(&kubeconfig, tracker, Some(params));
    runtime.start().await;
    Ok(())
}
