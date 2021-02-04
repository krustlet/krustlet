use k8s_openapi::api::core::v1::{Namespace, Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::Metadata;
use kube::api::{Api, DeleteParams, ListParams};

const E2E_NS_PREFIXES: &[&str] = &["wasi-e2e"];

#[tokio::main(threaded_scheduler)]
async fn main() -> anyhow::Result<()> {
    let result = smite_all_integration_test_pods().await;

    match &result {
        Ok(message) => println!("{}", message),
        Err(e) => println!("{}", e),
    };

    result.map(|_| ())
}

async fn smite_all_integration_test_pods() -> anyhow::Result<&'static str> {
    let client = match kube::Client::try_default().await {
        Ok(c) => c,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to acquire Kubernetes client: {}",
                e
            ))
        }
    };

    let namespaces = list_e2e_namespaces(client.clone()).await?;

    if namespaces.is_empty() {
        return Ok("No e2e namespaces found");
    }
    if !confirm_smite(&namespaces) {
        return Ok("Operation cancelled");
    }

    let pod_smite_operations = namespaces
        .iter()
        .map(|ns| smite_namespace_pods(client.clone(), ns));
    let pod_smite_results = futures::future::join_all(pod_smite_operations).await;
    let (_, pod_smite_errors) = pod_smite_results.partition_success();

    if !pod_smite_errors.is_empty() {
        return Err(smite_failure_error(&pod_smite_errors));
    }

    println!("Requested force-delete of all pods; requesting delete of namespaces...");

    let ns_smite_operations = namespaces
        .iter()
        .map(|ns| smite_namespace(client.clone(), ns));
    let ns_smite_results = futures::future::join_all(ns_smite_operations).await;
    let (_, ns_smite_errors) = ns_smite_results.partition_success();

    if !ns_smite_errors.is_empty() {
        return Err(smite_failure_error(&ns_smite_errors));
    }

    Ok("All e2e pods force-deleted; namespace cleanup may take a couple of minutes")
}

async fn list_e2e_namespaces(client: kube::Client) -> anyhow::Result<Vec<String>> {
    println!("Finding e2e namespaces...");

    let nsapi: Api<Namespace> = Api::all(client.clone());
    let nslist = nsapi.list(&ListParams::default()).await?;

    Ok(nslist
        .iter()
        .map(name_of)
        .filter(|n| is_e2e_namespace(n))
        .collect())
}

fn name_of(ns: &impl Metadata<Ty = ObjectMeta>) -> String {
    ns.metadata().name.as_ref().unwrap().to_owned()
}

fn is_e2e_namespace(namespace: &str) -> bool {
    E2E_NS_PREFIXES
        .iter()
        .any(|prefix| namespace.starts_with(prefix))
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
        return Err(smite_pods_failure_error(namespace, &errors));
    }

    Ok(())
}

async fn smite_pod(podapi: &Api<Pod>, pod: &Pod) -> anyhow::Result<()> {
    let pod_name = name_of(pod);
    let _ = podapi
        .delete(
            &pod_name,
            &DeleteParams {
                grace_period_seconds: Some(0),
                ..DeleteParams::default()
            },
        )
        .await?;
    Ok(())
}

async fn smite_namespace(client: kube::Client, namespace: &str) -> anyhow::Result<()> {
    let nsapi: Api<Namespace> = Api::all(client.clone());
    nsapi.delete(namespace, &DeleteParams::default()).await?;
    Ok(())
}

fn smite_failure_error(errors: &[anyhow::Error]) -> anyhow::Error {
    let message_list = errors
        .iter()
        .map(|e| format!("{}", e))
        .collect::<Vec<_>>()
        .join("\n");
    anyhow::anyhow!(
        "Some integration test resources were not cleaned up:\n{}",
        message_list
    )
}

fn smite_pods_failure_error(namespace: &str, errors: &[anyhow::Error]) -> anyhow::Error {
    let message_list = errors
        .iter()
        .map(|e| format!("  - {}", e))
        .collect::<Vec<_>>()
        .join("\n");
    anyhow::anyhow!(
        "- Namespace {}: pod delete(s) failed:\n{}",
        namespace,
        message_list
    )
}

fn confirm_smite(namespaces: &[String]) -> bool {
    println!(
        "Smite these namespaces and all resources within them: {}? (y/n) ",
        namespaces.join(", ")
    );
    let mut response = String::new();
    match std::io::stdin().read_line(&mut response) {
        Err(e) => {
            eprintln!("Error reading response: {}", e);
            confirm_smite(namespaces)
        }
        Ok(_) => response.starts_with('y') || response.starts_with('Y'),
    }
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
