use std::ops::Deref;
use std::path::Path;
use tokio::process::Child;

use tokio::sync::oneshot::{error::TryRecvError, Receiver};

const LOG_DIR: &str = "oneclick-logs";
const SOCKET_PATH: &str = "/tmp/csi.sock";

/// A struct for keeping references to processes and tempdirs. All embedded data and processes will
/// be cleaned up when dropped
pub(crate) struct CsiRunner {
    _processes: Vec<Child>,
    _signal: Receiver<Option<String>>,
    pub mock: super::MockCsiPlugin,
}

pub(crate) async fn launch_csi_things(node_name: &str) -> anyhow::Result<CsiRunner> {
    let bin_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("csi-test-binaries");
    let mut processes = Vec::with_capacity(3);

    let (mut signal, mock) = driver_start(node_name).await?;

    processes.push(external_provisioner_start(&bin_root)?);

    processes.push(registrar_start(&bin_root)?);

    // Check that something didn't crash
    match signal.try_recv() {
        Ok(None) => return Err(anyhow::anyhow!("Driver exited prematurely")),
        Ok(Some(msg)) => return Err(anyhow::anyhow!("Driver exited with error: {}", msg)),
        Err(TryRecvError::Closed) => {
            return Err(anyhow::anyhow!(
                "Sender was dropped, meaning the driver likely crashed"
            ))
        }
        Err(TryRecvError::Empty) => (),
    }

    Ok(CsiRunner {
        _processes: processes,
        _signal: signal,
        mock,
    })
}

/// Starts the mock CSI driver with the data dir set to a temporary
/// directory. Returns  a oneshot channel for checking if the process exited
async fn driver_start(
    node_name: &str,
) -> anyhow::Result<(Receiver<Option<String>>, super::MockCsiPlugin)> {
    // HACK: For some reason, implementing `Drop` on the socket isn't cleaning
    // up the socket on shutdown (probably because the socket is still running
    // when the `Drop` runs for the path), so this removes the path if it
    // already exists
    match tokio::fs::remove_file(SOCKET_PATH).await {
        Ok(_) => (),
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => (),
        Err(e) => return Err(e.into()),
    };
    let socket = super::grpc_sock::server::Socket::new(&SOCKET_PATH.to_string())
        .expect("unable to setup server listening on socket");

    let (tx, rx) = tokio::sync::oneshot::channel::<Option<String>>();

    let plugin = super::MockCsiPlugin::new(node_name);

    let owned_plugin = plugin.clone();
    tokio::spawn(async move {
        let serv = tonic::transport::Server::builder()
            .add_service(k8s_csi::v1_3_0::identity_server::IdentityServer::new(
                owned_plugin.clone(),
            ))
            .add_service(k8s_csi::v1_3_0::controller_server::ControllerServer::new(
                owned_plugin.clone(),
            ))
            .add_service(k8s_csi::v1_3_0::node_server::NodeServer::new(owned_plugin))
            .serve_with_incoming(socket);
        match serv.await {
            Ok(_) => tx.send(None),
            Err(e) => tx.send(Some(e.to_string())),
        }
    });

    println!("Mock CSI plugin started");

    Ok((rx, plugin))
}

fn external_provisioner_start(bin_root: &Path) -> anyhow::Result<Child> {
    let stdout = std::fs::File::create(Path::new(LOG_DIR).join("csi-provisioner.stdout"))?;
    let stderr = std::fs::File::create(Path::new(LOG_DIR).join("csi-provisioner.stderr"))?;

    let bin = bin_root.join("csi-provisioner");
    let kubeconfig = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Unable to find homedir"))?
        .join(".kube/config");

    let process = tokio::process::Command::new(bin)
        .args(&[
            "--csi-address",
            "/tmp/csi.sock",
            "--kubeconfig",
            kubeconfig.to_string_lossy().deref(),
        ])
        .stdout(stdout)
        .stderr(stderr)
        .kill_on_drop(true)
        .spawn()?;

    println!("CSI provisioner started");

    Ok(process)
}

fn registrar_start(bin_root: &Path) -> anyhow::Result<Child> {
    let stdout =
        std::fs::File::create(Path::new(LOG_DIR).join("csi-node-driver-registrar.stdout"))?;
    let stderr =
        std::fs::File::create(Path::new(LOG_DIR).join("csi-node-driver-registrar.stderr"))?;

    let bin = bin_root.join("csi-node-driver-registrar");
    let krustlet_plugin_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Unable to find homedir"))?
        .join(".krustlet/plugins");

    let process = tokio::process::Command::new(bin)
        .args(&[
            "--logtostderr",
            "--csi-address",
            "/tmp/csi.sock",
            "--kubelet-registration-path",
            "/tmp/csi.sock",
            "--plugin-registration-path",
            krustlet_plugin_dir.to_string_lossy().deref(),
        ])
        .stdout(stdout)
        .stderr(stderr)
        .kill_on_drop(true)
        .spawn()?;

    println!("Node driver registrar started");

    Ok(process)
}
