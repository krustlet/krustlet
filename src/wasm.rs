use crate::{
    kubelet::{Phase, Provider, Status},
    pod::{pod_status, KubePod},
};
use kube::client::APIClient;
use log::info;
use wasmtime::*;
use wasmtime_wasi::*;

/// WasmRuntime provides a Kubelet runtime implementation that executes WASM binaries.
#[derive(Clone)]
pub struct WasmRuntime {}

impl Provider for WasmRuntime {
    fn can_schedule(&self, pod: &KubePod) -> bool {
        // If there is a node selector and it has arch set to wasm32-wasi, we can
        // schedule it.
        let target_arch = "wasm32-wasi".to_string();
        pod.spec
            .node_selector
            .as_ref()
            .and_then(|i| {
                i.get("beta.kubernetes.io/arch")
                    .and_then(|v| Some(v.eq(&target_arch)))
            })
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
        let data = std::fs::read("./examples/greet.wasm")
            .expect("greet.wasm should be in examples directory");
        pod_status(client.clone(), pod.clone(), "Running", namespace.as_str());
        // TODO: Launch this in a thread.
        match wasm_run(&data) {
            Ok(_) => {
                info!("Pod run to completion");
                pod_status(client.clone(), pod, "Succeeded", namespace.as_str());
                Ok(())
            }
            Err(e) => {
                pod_status(client.clone(), pod, "Failed", namespace.as_str());
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
    fn status(&self, _pod: KubePod, _client: APIClient) -> Result<Status, failure::Error> {
        Ok(Status {
            phase: Phase::Succeeded,
            message: None,
        })
    }
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
pub fn wasm_run(data: &[u8]) -> Result<(), failure::Error> {
    let engine = HostRef::new(Engine::default());
    let store = HostRef::new(Store::new(&engine));
    let module = HostRef::new(Module::new(&store, data).expect("wasm module"));
    let preopen_dirs = vec![];
    let argv = vec![];
    let environ = vec![];
    // Build a list of WASI modules
    let wasi_inst = HostRef::new(create_wasi_instance(
        &store,
        &preopen_dirs,
        &argv,
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

    info!("Instance was executed");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::pod::KubePod;
    use k8s_openapi::api::core::v1::PodSpec;
    #[test]
    fn test_can_schedule() {
        let wr = WasmRuntime {};
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
            node_selector: Some(selector.clone()),
            ..Default::default()
        };
        assert!(!wr.can_schedule(&mock));
    }
}
