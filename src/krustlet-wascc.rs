use kubelet::config::Config;
use kubelet::module_store::FileModuleStore;
use kubelet::Kubelet;
use wascc_provider::WasccProvider;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The provider is responsible for all the "back end" logic. If you are creating
    // a new Kubelet, all you need to implement is a provider.
    let config = Config::new_from_flags(env!("CARGO_PKG_VERSION"));

    let kubeconfig = kubelet::tls_bootstrapper::bootstrap(&config.hostname).await?;

    // Initialize the logger
    env_logger::init();

    let client = oci_distribution::Client::default();
    let mut module_store_path = config.data_dir.join(".oci");
    module_store_path.push("modules");
    let store = FileModuleStore::new(client, &module_store_path);

    let provider = WasccProvider::new(store, &config, kubeconfig.clone()).await?;
    let kubelet = Kubelet::new(provider, kubeconfig, config).await?;
    kubelet.start().await
}
