#[macro_use]
extern crate failure;

use kube::client::APIClient;
use kubelet::pod::pod_status;
use kubelet::{pod::Pod, Phase, Provider, Status};
use log::{debug, info};
use std::collections::HashMap;
use wascc_host::{host, Actor, NativeCapability};

const ACTOR_PUBLIC_KEY: &str = "deislabs.io/wascc-action-key";
const TARGET_WASM32_WASCC: &str = "wasm32-wascc";

/// The name of the HTTP capability.
const HTTP_CAPABILITY: &str = "wascc:http_server";

#[cfg(target_os = "linux")]
const HTTP_LIB: &str = "./lib/libwascc_httpsrv.so";
#[cfg(target_os = "macos")]
const HTTP_LIB: &str = "./lib/libwascc_httpsrv.dylib";

/// Kubernetes' view of environment variables is an unordered map of string to string.
type EnvVars = std::collections::HashMap<String, String>;

/// WasccProvider provides a Kubelet runtime implementation that executes WASM binaries.
///
/// Currently, this runtime uses WASCC as a host, loading the primary container as an actor.
/// TODO: In the future, we will look at loading capabilities using the "sidecar" metaphor
/// from Kubernetes.
#[derive(Clone)]
pub struct WasccProvider {}

#[async_trait::async_trait]
impl Provider for WasccProvider {
    async fn init(&self) -> Result<(), failure::Error> {
        let data = NativeCapability::from_file(HTTP_LIB)
            .map_err(|e| format_err!("Failed to read HTTP capability {}: {}", HTTP_LIB, e))?;
        host::add_native_capability(data)
            .map_err(|e| format_err!("Failed to load HTTP capability: {}", e))
    }

    fn arch(&self) -> String {
        TARGET_WASM32_WASCC.to_string()
    }

    fn can_schedule(&self, pod: &Pod) -> bool {
        // If there is a node selector and it has arch set to wasm32-wascc, we can
        // schedule it.
        pod.spec
            .as_ref()
            .and_then(|s| s.node_selector.as_ref())
            .and_then(|i| {
                i.get("beta.kubernetes.io/arch")
                    .map(|v| v.eq(&TARGET_WASM32_WASCC))
            })
            .unwrap_or(false)
    }

    async fn add(&self, pod: Pod, client: APIClient) -> Result<(), failure::Error> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.
        debug!(
            "Pod added {:?}",
            pod.metadata.as_ref().and_then(|m| m.name.as_ref())
        );
        let namespace = pod
            .metadata
            .as_ref()
            .and_then(|m| m.namespace.as_deref())
            .unwrap_or_else(|| "default");
        // TODO: Replace with actual image store lookup when it is merged
        let data = std::fs::read("./testdata/echo.wasm")?;

        // TODO: Implement this for real.
        // Okay, so here is where things are REALLY unfinished. Right now, we are
        // only running the first container in a pod. And we are not using the
        // init containers at all. And they are not executed on their own threads.
        // So this is basically a toy.
        //
        // What it should do:
        // - for each volume
        //   - set up the volume map
        // - for each init container:
        //   - set up the runtime
        //   - mount any volumes (popen)
        //   - run it to completion
        //   - bail with an error if it fails
        // - for each container and ephemeral_container
        //   - set up the runtime
        //   - mount any volumes (popen)
        //   - run it to completion
        //   - bail if it errors
        let first_container = pod.spec.as_ref().map(|s| s.containers[0].clone()).unwrap();

        // This would lock us into one wascc actor per pod. I don't know if
        // that is a good thing. Other containers would then be limited
        // to acting as components... which largely follows the sidecar
        // pattern.
        //
        // Another possibility is to embed the key in the image reference
        // (image/foo.wasm@ed25519:PUBKEY). That might work best, but it is
        // not terribly useable.
        //
        // A really icky one would be to just require the pubkey in the env
        // vars and suck it out of there. But that violates the intention
        // of env vars, which is to communicate _into_ the runtime, not to
        // configure the runtime.
        let pubkey = pod
            .metadata
            .as_ref()
            .and_then(|s| s.annotations.as_ref())
            .unwrap()
            .get(ACTOR_PUBLIC_KEY)
            .map(|a| a.to_string())
            .unwrap_or_default();
        debug!("{:?}", pubkey);

        // TODO: Launch this in a thread. (not necessary with waSCC)
        let env = self.env_vars(client.clone(), &first_container, &pod).await;
        //let args = first_container.args.unwrap_or_else(|| vec![]);
        match wascc_run_http(data, env, pubkey.as_str()) {
            Ok(_) => {
                info!("Pod is executing on a thread");
                pod_status(client, &pod, "Running", namespace).await;
                Ok(())
            }
            Err(e) => {
                pod_status(client, &pod, "Failed", namespace).await;
                Err(failure::format_err!("Failed to run pod: {}", e))
            }
        }
    }

    async fn modify(&self, pod: Pod, _client: APIClient) -> Result<(), failure::Error> {
        // Modify will be tricky. Not only do we need to handle legitimate modifications, but we
        // need to sift out modifications that simply alter the status. For the time being, we
        // just ignore them, which is the wrong thing to do... except that it demos better than
        // other wrong things.
        info!("Pod modified");
        info!(
            "Modified pod spec: {}",
            serde_json::to_string_pretty(&pod.status.unwrap()).unwrap()
        );
        Ok(())
    }

    async fn delete(&self, pod: Pod, _client: APIClient) -> Result<(), failure::Error> {
        let pubkey = pod
            .metadata
            .unwrap_or_default()
            .annotations
            .unwrap_or_default()
            .get(ACTOR_PUBLIC_KEY)
            .map(|a| a.to_string())
            .unwrap_or_else(|| "".into());
        wascc_stop(&pubkey).map_err(|e| format_err!("Failed to stop wascc actor: {}", e))
    }

    async fn status(&self, pod: Pod, _client: APIClient) -> Result<Status, failure::Error> {
        match pod
            .metadata
            .unwrap_or_default()
            .annotations
            .unwrap_or_default()
            .get(ACTOR_PUBLIC_KEY)
        {
            None => Ok(Status {
                phase: Phase::Unknown,
                message: None,
            }),
            Some(pk) => {
                match host::actor_claims(pk) {
                    None => {
                        // FIXME: I don't know how to tell if an actor failed.
                        Ok(Status {
                            phase: Phase::Succeeded,
                            message: None,
                        })
                    }
                    Some(_) => Ok(Status {
                        phase: Phase::Running,
                        message: None,
                    }),
                }
            }
        }
    }
}

/// Run a WasCC module inside of the host, configuring it to handle HTTP requests.
///
/// This bootstraps an HTTP host, using the value of the env's `PORT` key to expose a port.
fn wascc_run_http(data: Vec<u8>, env: EnvVars, key: &str) -> Result<(), failure::Error> {
    let mut httpenv: HashMap<String, String> = HashMap::new();
    httpenv.insert(
        "PORT".into(),
        env.get("PORT")
            .map(|a| a.to_string())
            .unwrap_or_else(|| "80".to_string()),
    );

    wascc_run(
        data,
        key,
        vec![Capability {
            name: HTTP_CAPABILITY,
            env,
        }],
    )
}

/// Stop a running waSCC actor.
fn wascc_stop(key: &str) -> Result<(), wascc_host::errors::Error> {
    host::remove_actor(key)
}

/// Capability describes a waSCC capability.
///
/// Capabilities are made available to actors through a two-part processthread:
/// - They must be registered
/// - For each actor, the capability must be configured
struct Capability {
    name: &'static str,
    env: EnvVars,
}

/// Run the given WASM data as a waSCC actor with the given public key.
///
/// The provided capabilities will be configured for this actor, but the capabilities
/// must first be loaded into the host by some other process, such as register_native_capabilities().
fn wascc_run(
    data: Vec<u8>,
    key: &str,
    capabilities: Vec<Capability>,
) -> Result<(), failure::Error> {
    info!("wascc run");
    let load = Actor::from_bytes(data).map_err(|e| format_err!("Error loading WASM: {}", e))?;
    host::add_actor(load).map_err(|e| format_err!("Error adding actor: {}", e))?;

    capabilities.iter().try_for_each(|cap| {
        info!("configuring capability {}", cap.name);
        host::configure(key, cap.name, cap.env.clone())
            .map_err(|e| format_err!("Error configuring capabilities for module: {}", e))
    })?;
    info!("Instance executing");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use kubelet::pod::Pod;

    #[test]
    fn test_init() {
        let provider = WasccProvider {};
        provider.init().expect("HTTP capability is registered");
    }

    #[test]
    fn test_wascc_run() {
        // Open file
        let data = std::fs::read("./testdata/echo.wasm").expect("read the wasm file");
        // Send into wascc_run
        wascc_run_http(
            data,
            EnvVars::new(),
            "MB4OLDIC3TCZ4Q4TGGOVAZC43VXFE2JQVRAXQMQFXUCREOOFEKOKZTY2",
        )
        .expect("successfully executed a WASM");

        // Give the webserver a chance to start up.
        std::thread::sleep(std::time::Duration::from_secs(3));
        wascc_stop("MB4OLDIC3TCZ4Q4TGGOVAZC43VXFE2JQVRAXQMQFXUCREOOFEKOKZTY2")
            .expect("Removed the actor");
    }

    #[test]
    fn test_can_schedule() {
        let wr = WasccProvider {};
        let mut mock = KubePod {
            spec: Default::default(),
            metadata: Default::default(),
            status: Default::default(),
            types: Default::default(),
        };
        assert!(!wr.can_schedule(&mock));

        let mut selector = std::collections::BTreeMap::new();
        selector.insert(
            "beta.kubernetes.io/arch".to_string(),
            "wasm32-wascc".to_string(),
        );
        mock.spec = PodSpec {
            node_selector: Some(selector.clone()),
            ..Default::default()
        };
        assert!(wr.can_schedule(&mock));
        selector.insert("beta.kubernetes.io/arch".to_string(), "amd64".to_string());
        mock.spec = PodSpec {
            node_selector: Some(selector),
            ..Default::default()
        };
        assert!(!wr.can_schedule(&mock));
    }
}
