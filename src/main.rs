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
    // Read the environment. Note that this tries a KubeConfig file first, then
    // falls back on an in-cluster configuration.
    let kubeconfig = config::load_kube_config()
        .or_else(|_| config::incluster_config())
        .expect("kubeconfig failed to load");
    let client = APIClient::new(kubeconfig);
    let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into());

    // Initialize the logger
    env_logger::init();

    // Register as a node.
    create_node(client.clone());

    // Start updating the node lease periodically
    let update_client = client.clone();
    let node_updater = std::thread::spawn(move || {
        let sleep_interval = std::time::Duration::from_secs(10);
        loop {
            update_node(update_client.clone());
            std::thread::sleep(sleep_interval);
        }
    });

    // This informer listens for pod events.
    let pod_informer = std::thread::spawn(move || {
        let resource = Api::v1Pod(client.clone()).within(namespace.as_str());

        // OMG FIX ME NOW! We are not filtering only for pods that use wasm32-wasi!

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

    // The webserver handles a few callbacks from Kubernetes.
    start_webserver()?;
    node_updater.join().expect("node update thread crashed");
    pod_informer.join().expect("informer thread crashed");

    Ok(())
}

/// Handle each event on a pod
///
/// This is the event handler for the main Pod informer. It handles the
/// Added, Modified, and Deleted events for pods, and also has an error trap
/// for events that go sideways.
///
/// It will attempt to execute a WASM for each pod.
fn handle(client: APIClient, event: WatchEvent<KubePod>, namespace: String) {
    match event {
        WatchEvent::Added(p) => {
            // To run an Add event, we load the WASM, update the pod status to Running,
            // and then execute the WASM, passing in the relevant data.
            // When the pod finishes, we update the status to Succeeded unless it
            // produces an error, in which case we mark it Failed.
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
            // Modify will be tricky. Not only do we need to handle legitimate modifications, but we
            // need to sift out modifications that simply alter the status. For the time being, we
            // just ignore them, which is the wrong thing to do... except that it demos better than
            // other wrong things.
            info!("Pod modified");
            info!(
                "Modified pod spec: {}",
                serde_json::to_string_pretty(&p.status.unwrap()).unwrap()
            );
        }
        WatchEvent::Deleted(p) => {
            // It doesn't appear that VK does anything with delete operations on the event
            // stream, but I am seeing pod deletions hang indefinitely. So clearly I am
            // missing something.
            pod_status(client.clone(), p, "Succeeded", namespace.as_str());
            println!("Pod deleted")
        }
        WatchEvent::Error(e) => println!("Error: {}", e),
    }
}
