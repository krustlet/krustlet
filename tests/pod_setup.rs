use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{Api, ListParams, WatchEvent},
    runtime::Informer,
};

pub async fn wait_for_pod_ready(
    client: kube::Client,
    pod_name: &str,
    namespace: &str,
) -> anyhow::Result<()> {
    let api = Api::namespaced(client, namespace);
    let inf: Informer<Pod> = Informer::new(api).params(
        ListParams::default()
            .fields(&format!("metadata.name={}", pod_name))
            .timeout(30),
    );

    let mut watcher = inf.poll().await?.boxed();
    let mut went_ready = false;
    while let Some(event) = watcher.try_next().await? {
        match event {
            WatchEvent::Modified(o) => {
                let phase = o.status.unwrap().phase.unwrap();
                if phase == "Running" {
                    went_ready = true;
                    break;
                }
            }
            WatchEvent::Error(e) => {
                panic!("WatchEvent error: {:?}", e);
            }
            _ => {}
        }
    }

    assert!(went_ready, "pod never went ready");

    Ok(())
}

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
    let api = Api::namespaced(client.clone(), namespace);
    let inf: Informer<Pod> = Informer::new(api).params(
        ListParams::default()
            .fields(&format!("metadata.name={}", pod_name))
            .timeout(30),
    );

    let mut watcher = inf.poll().await?.boxed();
    let mut went_ready = false;
    while let Some(event) = watcher.try_next().await? {
        match event {
            WatchEvent::Modified(o) => {
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
            WatchEvent::Error(e) => {
                panic!("WatchEvent error: {:?}", e);
            }
            _ => {}
        }
    }

    assert!(went_ready, format!("pod {} never went ready", pod_name));

    Ok(())
}
