use k8s_openapi::api::core::v1::{ContainerState, ContainerStatus, Pod, PodStatus};
use kube::api::Api;

#[derive(Clone, Debug)]
pub enum Id<'a> {
    Init(&'a str),
    App(&'a str),
}

#[derive(Clone, Debug)]
pub enum Expect<'a> {
    IsTerminatedWith(&'a str),
    IsNotPresent,
}

pub type ContainerStatusExpectation<'a> = (Id<'a>, Expect<'a>);

pub trait Verifiable {
    fn verify_against(&self, pod_status: &PodStatus) -> anyhow::Result<()>;
}

impl<'a> Verifiable for ContainerStatusExpectation<'a> {
    fn verify_against(&self, pod_status: &PodStatus) -> anyhow::Result<()> {
        let (container_id, expectation) = self.clone();
        let (container_statuses, name) = match container_id {
            Id::Init(name) => (&pod_status.init_container_statuses, name),
            Id::App(name) => (&pod_status.container_statuses, name),
        };

        match expectation {
            Expect::IsNotPresent => verify_not_present(container_statuses, name),
            Expect::IsTerminatedWith(expected_message) => verify_terminated(container_statuses, name, expected_message),
        }
    }
}

fn verify_terminated(
    actual_statuses: &Option<Vec<ContainerStatus>>,
    container_name: &str,
    expected: &str,
) -> anyhow::Result<()> {
    match actual_statuses {
        None => Err(anyhow::anyhow!("Expected statuses section not present")),
        Some(statuses) => match statuses.iter().find(|s| s.name == container_name) {
            None => Err(anyhow::anyhow!(
                "Expected {} present but it wasn't",
                container_name
            )),
            Some(status) => match &status.state {
                None => Err(anyhow::anyhow!(
                    "Expected {} to have state but it didn't",
                    container_name
                )),
                Some(state) => verify_terminated_state(&state, container_name, expected),
            },
        },
    }
}

fn verify_terminated_state(
    actual_state: &ContainerState,
    container_name: &str,
    expected: &str,
) -> anyhow::Result<()> {
    match &actual_state.terminated {
        None => Err(anyhow::anyhow!(
            "Expected {} terminated but was not",
            container_name
        )),
        Some(term_state) => match &term_state.message {
            None => Err(anyhow::anyhow!(
                "Expected {} termination message was not set",
                container_name
            )),
            Some(message) => {
                if message == expected {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!(
                        "Expected {} termination message '{}' but was '{}'",
                        container_name,
                        expected,
                        message
                    ))
                }
            }
        },
    }
}

fn verify_not_present(
    actual_statuses: &Option<Vec<ContainerStatus>>,
    container_name: &str,
) -> anyhow::Result<()> {
    match actual_statuses {
        None => Ok(()),
        Some(statuses) => match statuses.iter().find(|s| s.name == container_name) {
            None => Ok(()),
            Some(_) => Err(anyhow::anyhow!(
                "Expected {} not present but it was",
                container_name
            )),
        },
    }
}

pub async fn assert_container_statuses(
    pods: &Api<Pod>,
    pod_name: &str,
    expectations: Vec<ContainerStatusExpectation<'_>>,
) -> anyhow::Result<()> {
    let pod = pods.get(pod_name).await?;

    let status = pod
        .status
        .ok_or(anyhow::anyhow!("Pod {} had no status", pod_name))?;

    for expectation in expectations {
        if let Err(e) = expectation.verify_against(&status) {
            assert!(
                false,
                "Pod {} status expectation failed: {}",
                pod_name,
                e.to_string()
            );
        }
    }

    Ok(())
}
