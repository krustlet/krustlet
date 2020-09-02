use k8s_openapi::api::core::v1::{Namespace, Pod};
use k8s_openapi::Metadata;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, DeleteParams, ListParams};

#[tokio::main(threaded_scheduler)]
async fn main() -> anyhow::Result<()> {
    let result = smite_all_integration_test_pods().await;

    if let Err(e) = &result {
        eprintln!("{}", e);
    } else {
        println!("All e2e pods force-deleted; namespace cleanup may take a couple of minutes");
    }

    result
}

async fn smite_all_integration_test_pods() -> anyhow::Result<()> {
    let client = match kube::Client::try_default().await {
        Ok(c) => c,
        Err(e) => return Err(anyhow::anyhow!("Failed to acquire Kubernetes client: {}", e)),
    };

    let namespaces = list_e2e_namespaces(client.clone()).await?;

    let smite_operations = namespaces.iter().map(|ns| smite_namespace_pods(client.clone(), ns));
    let smite_results = futures::future::join_all(smite_operations).await;
    let (_, errors) = smite_results.partition_success();

    if !errors.is_empty() {
        return Err(anyhow::anyhow!(smite_failure_message(&errors)))
    }


    Ok(())
}

async fn list_e2e_namespaces(client: kube::Client) -> anyhow::Result<Vec<String>> {
    println!("Finding e2e namespaces...");

    let nsapi: Api<Namespace> = Api::all(client.clone());
    let nslist = nsapi.list(&ListParams::default()).await?;

    Ok(nslist.iter().map(name_of).filter(|n| is_e2e_namespace(n)).collect())
}

fn name_of(ns: &impl Metadata<Ty = ObjectMeta>) -> String {
    ns.metadata().unwrap().name.as_ref().unwrap().to_owned()
}

fn is_e2e_namespace(namespace: &str) -> bool {
    namespace.starts_with("wascc-e2e") || namespace.starts_with("wasi-e2e")
}

async fn smite_namespace_pods(client: kube::Client, namespace: &str) -> anyhow::Result<()> {
    println!("Finding pods in namespace {}...", namespace);

    let podapi: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pods = podapi.list(&ListParams::default()).await?;

    println!("Deleting pods in namespace {}...", namespace);

    let delete_operations = pods.iter().map(|p| smite_pod(&podapi, p));
    let delete_results = futures::future::join_all(delete_operations).await;
    let (_, errors) = delete_results.partition_success();

    if !errors.is_empty() {
        return Err(anyhow::anyhow!(smite_pods_failure_message(namespace, &errors)))
    }

    Ok(())
}

async fn smite_pod(podapi: &Api<Pod>, pod: &Pod) -> anyhow::Result<()> {
    let pod_name = name_of(pod);
    let _ = podapi.delete(&pod_name, &DeleteParams { grace_period_seconds: Some(0), ..DeleteParams::default() }).await?;
    Ok(())
}

fn smite_failure_message(errors: &Vec<anyhow::Error>) -> String {
    let message_list = errors.iter().map(|e| format!("{}", e)).collect::<Vec<_>>().join("\n");
    format!("Some integration test resources were not cleaned up:\n{}", message_list)
}

fn smite_pods_failure_message(namespace: &str, errors: &Vec<anyhow::Error>) -> String {
    let message_list = errors.iter().map(|e| format!("  - {}", e)).collect::<Vec<_>>().join("\n");
    format!("- Namespace {}: pod delete(s) failed:\n{}", namespace, message_list)
}

// TODO: deduplicate with oneclick

trait ResultSequence {
    type SuccessItem;
    type FailureItem;
    fn partition_success(self) -> (Vec<Self::SuccessItem>, Vec<Self::FailureItem>);
}

impl<T, E: std::fmt::Debug> ResultSequence for Vec<Result<T, E>> {
    type SuccessItem = T;
    type FailureItem = E;
    fn partition_success(self) -> (Vec<Self::SuccessItem>, Vec<Self::FailureItem>) {
        let (success_results, error_results): (Vec<_>, Vec<_>) =
            self.into_iter().partition(|r| r.is_ok());
        let success_values = success_results.into_iter().map(|r| r.unwrap()).collect();
        let error_values = error_results
            .into_iter()
            .map(|r| r.err().unwrap())
            .collect();
        (success_values, error_values)
    }
}