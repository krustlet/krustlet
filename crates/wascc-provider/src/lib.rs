#[macro_use]
extern crate failure;

use kube::client::APIClient;
use kubelet::pod::{pod_status, KubePod};
use kubelet::{Phase, Provider, Status};
use log::info;
use std::collections::HashMap;
use wascc_host::{host, Actor, NativeCapability};

// Formerly used wasmtime as a runtime
use wasmtime::*;
use wasmtime_wasi::*;

const HTTP_CAPABILITY: &str = "wascc:http_server";
const ACTOR_PUBLIC_KEY: &str = "deislabs.io/wascc-action-key";
const TARGET_WASM32_WASI: &str = "wasm32-wasi";

type EnvVars = std::collections::HashMap<String, String>;

/// WasmProvider provides a Kubelet runtime implementation that executes WASM binaries.
///
/// Currently, this runtime uses WASCC as a host, loading the primary container as an actor.
/// TODO: In the future, we will look at loading capabilities using the "sidecar" metaphor
/// from Kubernetes.
#[derive(Clone)]
pub struct WasccProvider {}

impl Provider for WasccProvider {
    fn init(&self) -> Result<(), failure::Error> {
        let httplib = "./lib/libwascc_httpsrv.dylib";
        // The match is to unwrap an error from a thread and convert it to a type that
        // can cross the thread boundary. There is surely a better way.
        match NativeCapability::from_file(httplib) {
            Err(e) => Err(format_err!(
                "Failed to read HTTP capability {}: {}",
                httplib,
                e
            )),
            Ok(data) => match host::add_native_capability(data) {
                Err(e) => Err(format_err!("Failed to load HTTP capability: {}", e)),
                Ok(_) => Ok(()),
            },
        }
    }

    fn can_schedule(&self, pod: &KubePod) -> bool {
        // If there is a node selector and it has arch set to wasm32-wasi, we can
        // schedule it.
        let target_arch = TARGET_WASM32_WASI.to_string();
        pod.spec
            .node_selector
            .as_ref()
            .and_then(|i| i.get("beta.kubernetes.io/arch").map(|v| v.eq(&target_arch)))
            .unwrap_or(false)
    }
    fn add(&self, pod: KubePod, client: APIClient) -> Result<(), failure::Error> {
        // To run an Add event, we load the WASM, update the pod status to Running,
        // and then execute the WASM, passing in the relevant data.
        // When the pod finishes, we update the status to Succeeded unless it
        // produces an error, in which case we mark it Failed.
        info!("Pod added");
        let namespace = pod
            .metadata
            .clone()
            .namespace
            .unwrap_or_else(|| "default".into());
        // Start with a hard-coded WASM file
        //let data = std::fs::read("./examples/greet.wasm")
        //    .expect("greet.wasm should be in examples directory");
        let data = std::fs::read("./lib/greet_actor_signed.wasm")?;
        //pod_status(client.clone(), pod.clone(), "Running", namespace.as_str());

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
        let first_container = pod.spec.containers[0].clone();

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
            .annotations
            .get(ACTOR_PUBLIC_KEY)
            .map(|a| a.to_string())
            .unwrap_or_else(|| "".into());

        // TODO: Launch this in a thread. (not necessary with waSCC)
        let env = self.env_vars(client.clone(), &first_container, &pod);
        //let args = first_container.args.unwrap_or_else(|| vec![]);
        match wascc_run(&data, env, pubkey.as_str()) {
            Ok(_) => {
                info!("Pod is executing on a thread");
                pod_status(client, pod, "Running", namespace.as_str());
                Ok(())
            }
            Err(e) => {
                pod_status(client, pod, "Failed", namespace.as_str());
                Err(failure::format_err!("Failed to run pod: {}", e))
            }
        }
    }
    fn modify(&self, pod: KubePod, _client: APIClient) -> Result<(), failure::Error> {
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
    fn status(&self, pod: KubePod, _client: APIClient) -> Result<Status, failure::Error> {
        match pod.metadata.annotations.get(ACTOR_PUBLIC_KEY) {
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

pub fn wascc_run(data: &[u8], env: EnvVars, key: &str) -> Result<(), failure::Error> {
    let load = match Actor::from_bytes(data.to_vec()) {
        Err(e) => return Err(format_err!("Error loading WASM: {}", e.to_string())),
        Ok(data) => data,
    };
    if let Err(e) = host::add_actor(load) {
        return Err(format_err!("Error adding actor: {}", e.to_string()));
    }
    let mut httpenv: HashMap<String, String> = HashMap::new();
    httpenv.insert(
        "PORT".into(),
        env.get("PORT")
            .map(|a| a.to_string())
            .unwrap_or_else(|| "80".to_string()),
    );
    // TODO: Middleware provider for env vars
    match host::configure(key, HTTP_CAPABILITY, httpenv) {
        Err(e) => {
            return Err(format_err!(
                "Error configuring HTTP server for module: {}",
                e.to_string()
            ));
        }
        Ok(_) => {
            info!("Instance executing");
        }
    }
    Ok(())
}

/// Given a WASM binary, execute it.
///
/// Currently, this uses the wasmtime runtime with the WASI
/// module added.
///
/// TODO: This should be refactored into a struct where an
/// outside tool can set the dirs, args, and environment, and
/// then execute the WASM. It would be excellent to have a
/// convenience function that could take the pod spec and derive
/// all of this from that.
pub fn wasm_run(data: &[u8], env: EnvVars, args: Vec<String>) -> Result<(), failure::Error> {
    let engine = HostRef::new(Engine::default());
    let store = HostRef::new(Store::new(&engine));
    let module = HostRef::new(Module::new(&store, data).expect("wasm module"));
    let preopen_dirs = vec![];
    let mut environ = vec![];
    env.iter()
        .for_each(|item| environ.push((item.0.to_string(), item.1.to_string())));
    // Build a list of WASI modules
    let wasi_inst = HostRef::new(create_wasi_instance(
        &store,
        &preopen_dirs,
        &args,
        &environ,
    )?);
    // Iterate through the module includes and resolve imports
    let imports = module
        .borrow()
        .imports()
        .iter()
        .map(|i| {
            let module_name = i.module().as_str();
            let field_name = i.name().as_str();
            if let Some(export) = wasi_inst.borrow().find_export_by_name(field_name) {
                Ok(export.clone())
            } else {
                failure::bail!(
                    "Import {} was not found in module {}",
                    field_name,
                    module_name
                )
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Then create the instance
    let _instance = Instance::new(&store, &module, &imports).expect("wasm instance");
    info!("Instance executing");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::pod::KubePod;
    use k8s_openapi::api::core::v1::PodSpec;
    #[test]
    fn test_can_schedule() {
        let wr = WasmProvider {};
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
            "wasm32-wasi".to_string(),
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
