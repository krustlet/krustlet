use kubelet::config::Config;
use kubelet::plugin_watcher::PluginRegistry;
use kubelet::resources::DeviceManager;
use kubelet::store::composite::ComposableStore;
use kubelet::store::oci::FileStore;
use kubelet::Kubelet;
use std::convert::TryFrom;
use std::sync::Arc;
use wasi_provider::WasiProvider;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    // The provider is responsible for all the "back end" logic. If you are creating
    // a new Kubelet, all you need to implement is a provider.
    let config = Config::new_from_file_and_flags(env!("CARGO_PKG_VERSION"), None);

    // Initialize the logger
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let kubeconfig = kubelet::bootstrap(&config, &config.bootstrap_file, notify_bootstrap).await?;

    let store = make_store(&config);
    let plugin_registry = Arc::new(PluginRegistry::new(&config.plugins_dir));
    let device_plugin_manager = Arc::new(DeviceManager::new(
        &config.device_plugins_dir,
        kube::Client::try_from(kubeconfig.clone())?,
        &config.node_name,
    ));

    let provider = WasiProvider::new(
        store,
        &config,
        kubeconfig.clone(),
        plugin_registry,
        device_plugin_manager,
    )
    .await?;
    let kubelet = Kubelet::new(provider, kubeconfig, config).await?;
    kubelet.start().await
}

fn make_store(config: &Config) -> Arc<dyn kubelet::store::Store + Send + Sync> {
    let client = oci_distribution::Client::from_source(config);
    let mut store_path = config.data_dir.join(".oci");
    store_path.push("modules");
    let file_store = Arc::new(FileStore::new(client, &store_path));

    if config.allow_local_modules {
        file_store.with_override(Arc::new(kubelet::store::fs::FileSystemStore {}))
    } else {
        file_store
    }
}

fn notify_bootstrap(message: String) {
    println!("BOOTSTRAP: {}", message);
}
