use log::info;
use wasmtime::*;
use wasmtime_wasi::*;

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
