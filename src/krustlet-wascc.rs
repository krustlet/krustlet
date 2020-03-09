use kube::config;
use kubelet::config::Config;
use kubelet::Kubelet;
use wascc_provider::WasccProvider;

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
    let provider = WasccProvider {};
    let kubelet = Kubelet::new(
        provider,
        kubeconfig,
        Config::new_from_flags(env!("CARGO_PKG_VERSION")),
    );
    kubelet.start().await
}
