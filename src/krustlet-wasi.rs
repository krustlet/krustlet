use kube::config;
use kubelet::config::Config;
use kubelet::Kubelet;
use wasi_provider::WasiProvider;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Read the environment. Note that this tries a KubeConfig file first, then
    // falls back on an in-cluster configuration.
    let kubeconfig = config::load_kube_config()
        .await
        .or_else(|_| config::incluster_config())?;

    // Initialize the logger
    env_logger::init();

    // The provider is responsible for all the "back end" logic. If you are creating
    // a new Kubelet, all you need to implement is a provider.
    let conf = Config::new_from_flags(env!("CARGO_PKG_VERSION"));
    let client = oci_distribution::Client::default();
    let provider = WasiProvider::new(client, &conf.data_dir).await?;
    let kubelet = Kubelet::new(provider, kubeconfig, conf);
    kubelet.start().await
}
