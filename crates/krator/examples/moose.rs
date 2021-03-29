use k8s_openapi::Metadata;
use krator::{
    Manifest, ObjectState, ObjectStatus, Operator, OperatorRuntime, State, Transition, TransitionTo,
};
use kube::api::ListParams;
use kube_derive::CustomResource;
use rand::seq::IteratorRandom;
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use structopt::StructOpt;
use tokio::sync::RwLock;
use tracing::info;

#[derive(CustomResource, Debug, Serialize, Deserialize, Clone, Default, JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
enum MoosePhase {
    Asleep,
    Hungry,
    Roaming,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
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
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
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
        tokio::time::sleep(std::time::Duration::from_secs((state.food / 10.0) as u64)).await;
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
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
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

    #[cfg(feature = "admission-webhook")]
    async fn admission_hook(
        &self,
        manifest: Self::Manifest,
    ) -> krator::admission::AdmissionResult<Self::Manifest> {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
        // All moose names start with "M"
        let name = manifest.metadata().name.clone().unwrap();
        info!("Processing admission hook for moose named {}", name);
        match name.chars().next() {
            Some('m') | Some('M') => krator::admission::AdmissionResult::Allow(manifest),
            _ => krator::admission::AdmissionResult::Deny(Status {
                code: Some(400),
                message: Some("Mooses may only have names starting with 'M'.".to_string()),
                status: Some("Failure".to_string()),
                ..Default::default()
            }),
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "moose",
    about = "An example Operator for `Moose` custom resources."
)]
struct Opt {
    /// Send traces to Jaeger.
    /// Configure with the standard environment variables:
    /// https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/sdk-environment-variables.md#jaeger-exporter
    #[structopt(long)]
    jaeger: bool,
    /// Configure logger to emit JSON output.
    #[structopt(long)]
    json: bool,
}

fn init_logger() -> anyhow::Result<Option<opentelemetry_jaeger::Uninstall>> {
    // This isn't very DRY, but all of these combinations have different types,
    // and Boxing them doesn't seem to work.
    let opt = Opt::from_args();
    let guard = if opt.json {
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .json()
            .finish();
        if opt.jaeger {
            use tracing_subscriber::layer::SubscriberExt;
            let (tracer, _uninstall) = opentelemetry_jaeger::new_pipeline()
                .from_env()
                .with_service_name("moose_operator")
                .install()?;
            let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
            let subscriber = subscriber.with(telemetry);
            tracing::subscriber::set_global_default(subscriber)?;
            Some(_uninstall)
        } else {
            tracing::subscriber::set_global_default(subscriber)?;
            None
        }
    } else {
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .pretty()
            .finish();
        if opt.jaeger {
            use tracing_subscriber::layer::SubscriberExt;
            let (tracer, _uninstall) = opentelemetry_jaeger::new_pipeline()
                .from_env()
                .with_service_name("moose_operator")
                .install()?;
            let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
            let subscriber = subscriber.with(telemetry);
            tracing::subscriber::set_global_default(subscriber)?;
            Some(_uninstall)
        } else {
            tracing::subscriber::set_global_default(subscriber)?;
            None
        }
    };
    Ok(guard)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let _guard = init_logger()?;

    let kubeconfig = kube::Config::infer().await?;
    let tracker = MooseTracker::new();

    info!("crd:\n{}", serde_yaml::to_string(&Moose::crd()).unwrap());

    // Only track mooses in Glacier NP
    let params = ListParams::default().labels("nps.gov/park=glacier");

    let mut runtime = OperatorRuntime::new(&kubeconfig, tracker, Some(params));
    runtime.start().await;
    Ok(())
}
