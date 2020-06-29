use kubelet::config::Config;
use kubelet::store::oci::FileStore;
use kubelet::Kubelet;
use wasi_provider::WasiProvider;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The provider is responsible for all the "back end" logic. If you are creating
    // a new Kubelet, all you need to implement is a provider.
    let config = Config::new_from_file_and_flags(env!("CARGO_PKG_VERSION"), None);

    // Initialize the logger
    env_logger::init();

    let kubeconfig = kubelet::bootstrap(&config, &config.bootstrap_file).await?;

    let client = oci_distribution::Client::default();
    let mut store_path = config.data_dir.join(".oci");
    store_path.push("modules");
    let store = FileStore::new(client, &store_path);

    let provider = WasiProvider::new(store, &config, kubeconfig.clone()).await?;
    let kubelet = Kubelet::new(provider, kubeconfig, config).await?;
    kubelet.start().await
}
