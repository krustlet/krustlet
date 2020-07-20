use futures::{TryStreamExt};
use k8s_openapi::api::core::v1::{Pod};
use kube::{
    api::{Api, LogParams},
};

pub async fn pod_log_equals(
    pods: &Api<Pod>,
    pod_name: &str,
    expected_log: &str,
) -> anyhow::Result<()> {
    let mut logs = pods.log_stream(pod_name, &LogParams::default()).await?;

    while let Some(chunk) = logs.try_next().await? {
        assert_eq!(expected_log, String::from_utf8_lossy(&chunk));
    }

    Ok(())
}

pub async fn pod_log_contains(
    pods: &Api<Pod>,
    pod_name: &str,
    expected_log: &str,
) -> anyhow::Result<()> {
    let logs = pods.logs(pod_name, &LogParams::default()).await?;
    assert!(
        logs.contains(expected_log),
        format!("Expected log containing {} but got {}", expected_log, logs)
    );
    Ok(())
}

pub async fn pod_container_log_contains(
    pods: &Api<Pod>,
    pod_name: &str,
    container_name: &str,
    expected_log: &str,
) -> anyhow::Result<()> {
    let mut log_params = LogParams::default();
    log_params.container = Some(container_name.to_owned());
    let logs = pods.logs(pod_name, &log_params).await?;
    assert!(
        logs.contains(expected_log),
        format!("Expected log containing {} but got {}", expected_log, logs)
    );
    Ok(())
}

// pub async fn pod_log_does_not_contain(
//     pods: &Api<Pod>,
//     pod_name: &str,
//     unexpected_log: &str,
// ) -> anyhow::Result<()> {
//     let logs = pods.logs(pod_name, &LogParams::default()).await?;
//     assert!(
//         !logs.contains(unexpected_log),
//         format!(
//             "Expected log NOT containing {} but got {}",
//             unexpected_log, logs
//         )
//     );
//     Ok(())
// }

pub async fn pod_exited_successfully(pods: &Api<Pod>, pod_name: &str) -> anyhow::Result<()> {
    let pod = pods.get(pod_name).await?;

    let state = (|| {
        pod.status?.container_statuses?[0]
            .state
            .as_ref()?
            .terminated
            .clone()
    })()
    .expect("Could not fetch terminated states");
    assert_eq!(state.exit_code, 0);

    Ok(())
}

pub async fn pod_exited_with_failure(pods: &Api<Pod>, pod_name: &str) -> anyhow::Result<()> {
    let pod = pods.get(pod_name).await?;

    let phase = (|| pod.status?.phase)().expect("Could not get pod phase");
    assert_eq!(phase, "Failed");

    Ok(())
}

pub async fn pod_message_contains(
    pods: &Api<Pod>,
    pod_name: &str,
    expected_message: &str,
) -> anyhow::Result<()> {
    let pod = pods.get(pod_name).await?;

    let message = (|| pod.status?.message)().expect("Could not get pod message");
    assert!(
        message.contains(expected_message),
        format!(
            "Expected pod message containing {} but got {}",
            expected_message, message
        )
    );

    Ok(())
}

pub async fn main_container_exited_with_failure(
    pods: &Api<Pod>,
    pod_name: &str,
) -> anyhow::Result<()> {
    let pod = pods.get(pod_name).await?;

    let state = (|| {
        pod.status?.container_statuses?[0]
            .state
            .as_ref()?
            .terminated
            .clone()
    })()
    .expect("Could not fetch terminated states");
    assert_eq!(state.exit_code, 1);

    Ok(())
}

pub async fn container_file_contains(
    container_file_path: &str,
    expected_content: &str,
    file_error: &str,
) -> anyhow::Result<()> {
    let file_path_base = dirs::home_dir()
        .expect("home dir does not exist")
        .join(".krustlet/volumes/hello-wasi-default");  // TODO: volume name
    let container_file_bytes = tokio::fs::read(file_path_base.join(container_file_path))
        .await
        .expect(file_error);
    assert_eq!(
        expected_content.to_owned().into_bytes(),
        container_file_bytes
    );
    Ok(())
}
