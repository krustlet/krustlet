use env_logger;
use kube::{
    api::{Api, Informer, WatchEvent},
    client::APIClient,
    config,
};
use log::{error, info};

use krustlet::{
    node::{create_node, update_node},
    pod::{pod_status, KubePod},
    server::start_webserver,
    wasm::wasm_run,
};

fn main() -> Result<(), failure::Error> {
    let kubeconfig = config::load_kube_config()
        .or_else(|_| config::incluster_config())
        .expect("kubeconfig failed to load");
    let client = APIClient::new(kubeconfig);
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());

    env_logger::init();

    // Register as a node.
    create_node(client.clone());

    // Start updating the node periodically
    let update_client = client.clone();
    let node_updater = std::thread::spawn(move || {
        let sleep_interval = std::time::Duration::from_secs(10);
        loop {
            update_node(update_client.clone());
            std::thread::sleep(sleep_interval);
        }
    });

    let pod_informer = std::thread::spawn(move || {
        let resource = Api::v1Pod(client.clone()).within(namespace.as_str());

        // Create our informer and start listening.
        let informer = Informer::new(resource)
            .init()
            .expect("informer init failed");
        loop {
            informer.poll().expect("informer poll failed");
            while let Some(event) = informer.pop() {
                handle(client.clone(), event, namespace.clone());
            }
        }
    });

    start_webserver()?;

    node_updater.join().expect("node update thread crashed");

    pod_informer.join().expect("informer thread crashed");

    Ok(())
}
/// Handle each event on a pod
fn handle(client: APIClient, event: WatchEvent<KubePod>, namespace: String) {
    match event {
        WatchEvent::Added(p) => {
            info!("Pod added");
            // Start with a hard-coded WASM file
            let data = std::fs::read("./examples/greet.wasm")
                .expect("greet.wasm should be in examples directory");
            pod_status(client.clone(), p.clone(), "Running", namespace.as_str());
            match wasm_run(&data) {
                Ok(_) => {
                    info!("Pod run to completion");
                    pod_status(client.clone(), p, "Succeeded", namespace.as_str());
                }
                Err(e) => {
                    error!("Failed to run pod: {}", e);
                    pod_status(client.clone(), p, "Failed", namespace.as_str());
                }
            }
        }
        WatchEvent::Modified(p) => {
            info!("Pod modified");
            info!(
                "Modified pod spec: {}",
                serde_json::to_string_pretty(&p.status.unwrap()).unwrap()
            );
        }
        WatchEvent::Deleted(p) => {
            pod_status(client.clone(), p, "Succeeded", namespace.as_str());
            println!("Pod deleted")
        }
        WatchEvent::Error(e) => println!("Error: {}", e),
    }
}
