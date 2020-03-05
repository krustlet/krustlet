use kube::config;
use kubelet::config::get_from_flags;
use kubelet::Kubelet;
use wasi_provider::WasiProvider;

#[tokio::main]
async fn main() -> Result<(), failure::Error> {
    // Read the environment. Note that this tries a KubeConfig file first, then
    // falls back on an in-cluster configuration.
    let kubeconfig = config::load_kube_config()
        .await
        .or_else(|_| config::incluster_config())
        .expect("kubeconfig failed to load");

    // Initialize the logger
    env_logger::init();

    // The provider is responsible for all the "back end" logic. If you are creating
    // a new Kubelet, all you need to implement is a provider.
    let provider = WasiProvider::default();
    let kubelet = Kubelet::new(provider, kubeconfig, get_from_flags());
    kubelet.start().await
}
