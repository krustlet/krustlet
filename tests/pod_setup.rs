use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, ListParams};
use kube_runtime::watcher::{watcher, Event};

#[derive(PartialEq)]
pub enum OnFailure {
    Accept,
    Panic,
}

pub async fn wait_for_pod_complete(
    client: kube::Client,
    pod_name: &str,
    namespace: &str,
    on_failure: OnFailure,
) -> anyhow::Result<()> {
    let api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let inf = watcher(
        api,
        ListParams::default()
            .fields(&format!("metadata.name={}", pod_name))
            .timeout(30),
    );

    let mut watcher = inf.boxed();
    let mut went_ready = false;
    while let Some(event) = watcher.try_next().await? {
        if let Event::Applied(o) = event {
            let phase = o.status.unwrap().phase.unwrap();
            if phase == "Failed" && on_failure == OnFailure::Accept {
                return Ok(());
            }
            if phase == "Running" {
                went_ready = true;
            }
            if phase == "Succeeded" && !went_ready {
                panic!(
                    "Pod {} reached completed phase before receiving Running phase",
                    pod_name
                );
            } else if phase == "Succeeded" {
                break;
            }
        }
    }

    assert!(went_ready, "pod {} never went ready", pod_name);

    Ok(())
}
