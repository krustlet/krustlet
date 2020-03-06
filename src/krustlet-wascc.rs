use kube::config;
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
    //let client = APIClient::new(kubeconfig);
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());
    let address = std::env::var("POD_IP")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()?;

    // Initialize the logger
    env_logger::init();

    // The provider is responsible for all the "back end" logic. If you are creating
    // a new Kubelet, all you need to implement is a provider.
    let provider = WasccProvider {};
    let kubelet = Kubelet::new(provider, kubeconfig, namespace);
    kubelet.start(address).await
}
