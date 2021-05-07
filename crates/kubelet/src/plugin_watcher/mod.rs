//! The Kubelet plugin manager. Used to lookup which plugins are registered with this node.
use crate::fs_watch::FileSystemWatcher;
use crate::grpc_sock;
use crate::plugin_registration_api::v1::{
    registration_client::RegistrationClient, InfoRequest, PluginInfo, RegistrationStatus,
    API_VERSION,
};

use anyhow::Context;
use notify::Event;
use tokio::fs::{create_dir_all, read_dir};
use tokio::sync::{RwLock, RwLockWriteGuard};
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;
use tonic::Request;
use tracing::{debug, error, instrument, trace, warn};
use tracing_futures::Instrument;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};

#[cfg(target_family = "unix")]
const DEFAULT_PLUGIN_PATH: &str = "/var/lib/kubelet/plugins_registry/";
#[cfg(target_family = "windows")]
const DEFAULT_PLUGIN_PATH: &str = "c:\\ProgramData\\kubelet\\plugins_registry";

const SOCKET_EXTENSION: &str = "sock";
const ALLOWED_PLUGIN_TYPES: &[PluginType] = &[PluginType::CsiPlugin];

/// An enum for capturing possible plugin types. This is purely for clarity and capturing this
/// information is a compiled type as the information we get from gRPC is a string
#[derive(Debug, PartialEq)]
enum PluginType {
    CsiPlugin,
    DevicePlugin,
}

impl TryFrom<&str> for PluginType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "CSIPlugin" => Ok(PluginType::CsiPlugin),
            "DevicePlugin" => Ok(PluginType::DevicePlugin),
            _ => Err(anyhow::anyhow!(
                "Unknown plugin type {}. Allowed types are 'CSIPlugin' and 'DevicePlugin'",
                value
            )),
        }
    }
}

/// Internal storage structure for a plugin
#[derive(Debug)]
struct PluginEntry {
    plugin_path: PathBuf,
    endpoint: Option<PathBuf>,
}

/// An internal storage plugin registry that implements most the same functionality as the [plugin
/// manager](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/pluginmanager)
/// in kubelet
pub struct PluginRegistry {
    plugins: RwLock<HashMap<String, PluginEntry>>,
    plugin_dir: PathBuf,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        PluginRegistry {
            plugin_dir: PathBuf::from(DEFAULT_PLUGIN_PATH),
            plugins: RwLock::new(HashMap::new()),
        }
    }
}

impl PluginRegistry {
    /// Returns a new plugin registrar configured with the given plugin directory path
    pub fn new<P: AsRef<Path>>(plugin_dir: P) -> Self {
        PluginRegistry {
            plugin_dir: PathBuf::from(plugin_dir.as_ref()),
            ..Default::default()
        }
    }

    /// Gets the endpoint for the given plugin name, returning `None` if it doesn't exist
    // TODO: Remove clippy exception when CSI is completed.
    #[allow(dead_code)]
    pub async fn get_endpoint(&self, plugin_name: &str) -> Option<PathBuf> {
        let plugins = self.plugins.read().await;
        plugins
            .get(plugin_name)
            .map(|v| v.endpoint.as_ref().unwrap_or(&v.plugin_path).to_owned())
    }

    /// Starts the plugin registrar and runs all automatic plugin discovery and registration loops.
    /// This will block indefinitely or until the underlying watch stops. To stop watching the
    /// filesystem, simply stop polling the future. Underneath the hood this is creating a watch on
    /// a directory using OS native APIs and then watching that event stream
    pub async fn run(&self) -> anyhow::Result<()> {
        // Create plugin directory if it doesn't exist
        create_dir_all(&self.plugin_dir).await?;

        // Walk the plugin dir beforehand and process any currently existing files
        let dir_entries: Vec<PathBuf> = ReadDirStream::new(read_dir(&self.plugin_dir).await?)
            .map(|res| res.map(|entry| entry.path()))
            .collect::<Result<Vec<PathBuf>, _>>()
            .await?;

        // Manually assemble an event and call handle_event to reconfigure sockets properly on
        // restart. We are looping again here so we can just log an error and continue processing if
        // there was a failure loading a plugin
        for dir in dir_entries.into_iter() {
            if let Err(e) = self
                .handle_create(Event {
                    paths: vec![dir.clone()],
                    ..Default::default()
                })
                .await
            {
                error!(error = %e, path = %dir.display(), "Unable to load plugin")
            }
        }

        let mut event_stream = FileSystemWatcher::new(&self.plugin_dir)?;

        while let Some(res) = event_stream.next().await {
            match res {
                Ok(event) if event.kind.is_create() => {
                    if let Err(e) = self.handle_create(event).await {
                        error!(error = %e, "An error occurred while processing a new plugin");
                    }
                }
                Ok(event) if event.kind.is_remove() => self.handle_delete(event).await,
                // Skip any events that aren't create or delete
                Ok(_) => continue,
                Err(e) => {
                    error!(error = %e, "An error occurred while watching the plugin directory. Will continue to retry")
                }
            }
        }
        Ok(())
    }

    async fn handle_create(&self, event: Event) -> anyhow::Result<()> {
        for discovered_path in plugin_paths(event.paths) {
            async {
                debug!(
                    "Beginning plugin registration for discovered plugin"
                );

                // Step 1: Attempt to call the socket. If this fails, we don't have any guarantee we'll
                // be able to inform it we've failed. So just unwrap the error here
                let plugin_info = get_plugin_info(&discovered_path).await?;
                tracing::Span::current().record("plugin_info", &tracing::field::debug(&plugin_info));
                debug!(
                    "Successfully retrieved information for discovered plugin"
                );

                // Step 2: Validate discovered data
                if let Err(e) = self.validate(&plugin_info, &discovered_path).await {
                    inform_plugin(&discovered_path, Some(e.to_string())).await?;
                    return Err(e).with_context(|| {
                        format!(
                            "Validation step failed for plugin discovered at {}",
                            discovered_path.display()
                        )
                    });
                }
                debug!(
                    "Successfully validated discovered plugin"
                );

                // Step 3: Register plugin to local storage
                self.register(&plugin_info, &discovered_path).await;

                // Step 4: Inform plugin
                inform_plugin(&discovered_path, None).await?;
                debug!("Plugin registration complete");
                Ok(())
            }.instrument(tracing::trace_span!("plugin_registration", path = %discovered_path.display(), plugin_info = tracing::field::Empty)).await?;
        }
        Ok(())
    }

    async fn handle_delete(&self, event: Event) {
        let mut plugins = self.plugins.write().await;
        for deleted_plugin in plugin_paths(event.paths) {
            remove_plugin(&mut plugins, deleted_plugin);
        }
    }

    /// Registers the plugin in our HashMap
    async fn register(&self, info: &PluginInfo, discovered_path: &Path) {
        let mut lock = self.plugins.write().await;
        lock.insert(
            info.name.clone(),
            PluginEntry {
                plugin_path: discovered_path.to_owned(),
                endpoint: match info.endpoint.is_empty() {
                    true => None,
                    false => Some(PathBuf::from(&info.endpoint)),
                },
            },
        );
    }

    /// Validates the given plugin info gathered from a discovered plugin, returning an error with
    /// additional information if it is not valid. This will validate 3 specific things (should
    /// answer YES to all of these):
    /// 1. Is it a CSIPlugin? If it isn't we will deny it for now. This will be removed as we
    ///    iterate and if it is needed
    /// 2. Does the list of supported versions contain the version we expect?
    /// 3. Is the plugin name available? 3a. If the name is already registered, is the endpoint the
    ///    exact same? If it is, we allow it to reregister
    #[instrument(level = "info", skip(self))]
    async fn validate(&self, info: &PluginInfo, discovered_path: &Path) -> anyhow::Result<()> {
        trace!("Starting validation for plugin");

        self.validate_plugin_type(info.r#type.as_str())?;
        trace!("Type validation complete");

        trace!("Checking supported versions");
        self.validate_plugin_version(&info.supported_versions)?;
        trace!("Supported version check complete");

        trace!("Checking for naming collisions");
        self.validate_is_unique(info, discovered_path).await?;
        trace!("Naming collision check complete");

        Ok(())
    }

    // Individual validation steps

    /// Check for valid type and if it is a CSIPlugin
    fn validate_plugin_type(&self, plugin_type: &str) -> anyhow::Result<()> {
        let plugin_type = PluginType::try_from(plugin_type)?;
        if !is_allowed_plugin_type(plugin_type) {
            warn!("DevicePlugins are not currently supported");
            return Err(anyhow::anyhow!("DevicePlugins are not currently supported"));
        }
        Ok(())
    }

    /// Check if we support one of the plugin's requested versions
    fn validate_plugin_version(&self, supported_versions: &[String]) -> anyhow::Result<()> {
        if !supported_versions.iter().any(|s| s == API_VERSION) {
            return Err(anyhow::anyhow!(
                "Plugin doesn't support version {}",
                API_VERSION
            ));
        }
        Ok(())
    }

    /// Validates if the plugin is unique (meaning it doesn't exist in the store) or, if it is the
    /// same, is the exact same endpoint configuration
    async fn validate_is_unique(
        &self,
        info: &PluginInfo,
        discovered_path: &Path,
    ) -> anyhow::Result<()> {
        let plugins = self.plugins.read().await;

        if let Some(current_path) = plugins.get(&info.name) {
            // If there is an endpoint set, use that to check, otherwise, use the discovered path
            if !info.endpoint.is_empty()
                && Some(PathBuf::from(&info.endpoint)) != current_path.endpoint
            {
                return Err(anyhow::format_err!(
                    "Plugin already exists with an endpoint of {:?}, which differs from the new endpoint of {}",
                    current_path.endpoint,
                    info.endpoint
                ));
            } else if *discovered_path != current_path.plugin_path {
                return Err(anyhow::anyhow!(
                    "Plugin already exists with an endpoint of {}, which differs from the new endpoint of {}",
                    current_path.plugin_path.display(),
                    discovered_path.display()
                ));
            }
        }

        Ok(())
    }
}

/// A helper function to clarify code intent when removing a plugin. This puts all the iterating and
/// stuff into a well-named place
fn remove_plugin(
    plugins: &mut RwLockWriteGuard<HashMap<String, PluginEntry>>,
    deleted_plugin: PathBuf,
) {
    let key = match plugins
        .iter()
        .find(|(_, v)| *v.plugin_path == deleted_plugin)
    {
        // Take ownership of the key to avoid an immutable borrow
        Some((key, _)) => key.to_owned(),
        // If for some reason it is already gone, no need to error
        None => return,
    };
    plugins.remove(&key);
}

// An allow list check for currently supported plugin types
fn is_allowed_plugin_type(t: PluginType) -> bool {
    ALLOWED_PLUGIN_TYPES.iter().any(|item| *item == t)
}

/// Attempts a `GetInfo` gRPC call to the endpoint to the path given
#[instrument(level = "info")]
async fn get_plugin_info(path: &Path) -> anyhow::Result<PluginInfo> {
    trace!("Connecting to plugin for GetInfo");
    let chan = grpc_sock::client::socket_channel(path).await?;
    let mut client = RegistrationClient::new(chan);

    let req = Request::new(InfoRequest {});

    trace!("Calling GetInfo");
    client
        .get_info(req)
        .await
        .map(|resp| resp.into_inner())
        .map_err(|status| {
            anyhow::anyhow!(
                "GetInfo call to {} failed with error code {} and message {}",
                path.display(),
                status.code(),
                status.message()
            )
        })
}

/// Informs the plugin at the given path of registration success or error. If the error parameter is
/// `None`, it will report as successful, otherwise the error message contained in the `Option` will
/// be sent to the plugin and it will be marked as failed
#[instrument(level = "info")]
async fn inform_plugin(path: &Path, error: Option<String>) -> anyhow::Result<()> {
    trace!("Connecting to plugin for NotifyRegistrationStatus");
    let chan = grpc_sock::client::socket_channel(path).await?;
    let mut client = RegistrationClient::new(chan);

    let req = Request::new(RegistrationStatus {
        plugin_registered: error.is_none(),
        error: error.unwrap_or_else(String::new),
    });

    trace!("Calling NotifyRegistrationStatus");
    client
        .notify_registration_status(req)
        .await
        .map_err(|status| {
            anyhow::anyhow!(
                "NotifyRegistrationStatus call to {} failed with error code {} and message {}",
                path.display(),
                status.code(),
                status.message()
            )
        })?;
    Ok(())
}

fn plugin_paths(paths: Vec<PathBuf>) -> impl Iterator<Item = PathBuf> {
    // Filter paths, checking if it is a socket and not a directory. Why not check if it is a
    // file? Because it isn't technically a regular file and so the `is_file` check returns
    // false
    paths
        .into_iter()
        .filter(|p| !p.is_dir() && p.extension().unwrap_or_default() == SOCKET_EXTENSION)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::plugin_registration_api::v1::{
        registration_server::{Registration, RegistrationServer},
        InfoRequest, PluginInfo, RegistrationStatusResponse, API_VERSION,
    };

    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc::{self, Receiver, Sender};
    use tokio::sync::Mutex;
    use tokio::time::timeout;
    #[cfg(target_family = "windows")]
    use tokio_compat_02::FutureExt;
    use tonic::{transport::Server, Request, Response, Status};

    const FAKE_ENDPOINT: &str = "/tmp/foo.sock";

    ////////////////////////////////////////////////////////////////////////
    //////////////////////// BEGIN test scaffolding ////////////////////////
    ////////////////////////////////////////////////////////////////////////

    #[derive(Debug)]
    struct TestCsiPlugin {
        name: String,
        registration_response: Mutex<Sender<RegistrationStatus>>,
    }

    #[tonic::async_trait]
    impl Registration for TestCsiPlugin {
        async fn get_info(
            &self,
            _req: Request<InfoRequest>,
        ) -> Result<Response<PluginInfo>, Status> {
            Ok(Response::new(PluginInfo {
                r#type: "CSIPlugin".to_string(),
                name: self.name.clone(),
                endpoint: FAKE_ENDPOINT.to_string(),
                supported_versions: vec![API_VERSION.to_string()],
            }))
        }

        async fn notify_registration_status(
            &self,
            req: Request<RegistrationStatus>,
        ) -> Result<Response<RegistrationStatusResponse>, Status> {
            self.registration_response
                .lock()
                .await
                .send(req.into_inner())
                .await
                .expect("should be able to send registration status on channel");

            Ok(Response::new(RegistrationStatusResponse {}))
        }
    }

    #[derive(Debug)]
    // A plugin that always fails GetInfo
    struct InvalidCsiPlugin {
        name: String,
        registration_response: Mutex<Sender<RegistrationStatus>>,
    }

    #[tonic::async_trait]
    impl Registration for InvalidCsiPlugin {
        async fn get_info(
            &self,
            _req: Request<InfoRequest>,
        ) -> Result<Response<PluginInfo>, Status> {
            Ok(Response::new(PluginInfo {
                r#type: "CSIPlugin".to_string(),
                name: self.name.clone(),
                endpoint: FAKE_ENDPOINT.to_string(),
                supported_versions: vec!["nope".to_string()],
            }))
        }

        async fn notify_registration_status(
            &self,
            req: Request<RegistrationStatus>,
        ) -> Result<Response<RegistrationStatusResponse>, Status> {
            self.registration_response
                .lock()
                .await
                .send(req.into_inner())
                .await
                .expect("should be able to send registration status on channel");

            Ok(Response::new(RegistrationStatusResponse {}))
        }
    }

    /// Setup the test, returning the temporary directory (as we need it around to keep it from
    /// dropping) and a PluginRegistry configured with the path. The PluginRegistry is wrapped in
    /// an Arc to facilitate easy moving to a task using tokio::spawn
    fn setup() -> (tempfile::TempDir, Arc<PluginRegistry>) {
        let tempdir = tempfile::tempdir().expect("should be able to create tempdir");

        let registrar = PluginRegistry::new(&tempdir);

        (tempdir, Arc::new(registrar))
    }

    async fn setup_server(plugin: impl Registration, path: impl AsRef<Path>) {
        let socket = grpc_sock::server::Socket::new(&path)
            .expect("unable to setup server listening on socket");

        tokio::spawn(async move {
            let serv = Server::builder()
                .add_service(RegistrationServer::new(plugin))
                .serve_with_incoming(socket);
            #[cfg(target_family = "windows")]
            let serv = serv.compat();
            serv.await.expect("Unable to serve test plugin");
            // Print this out in case of failure for ease of debugging
            println!("server exited");
        });
    }

    // Starts the registrar and waits for a little bit of time for it to start running
    async fn start_registrar(registrar: Arc<PluginRegistry>) {
        tokio::spawn(async move {
            registrar
                .run()
                .await
                .expect("registrar didn't run successfully");
            // Print this out in case of failure for ease of debugging
            println!("registrar exited");
        });

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Waits with a timeout on getting a RegistrationStatus. Will unwrap all errors
    async fn get_registration_response(mut rx: Receiver<RegistrationStatus>) -> RegistrationStatus {
        timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("timed out while waiting for registration status response")
            .expect("Should have received a valid response in the channel")
    }

    ////////////////////////////////////////////////////////////////////////
    /////////////////////// END test scaffolding ///////////////////////////
    ////////////////////////////////////////////////////////////////////////

    #[tokio::test]
    async fn test_successful_registration() {
        let (tempdir, registrar) = setup();

        let (tx, rx) = mpsc::channel(1);

        let plugin = TestCsiPlugin {
            name: "foo".to_string(),
            registration_response: Mutex::new(tx),
        };

        start_registrar(registrar.clone()).await;

        setup_server(plugin, tempdir.path().join("foo.sock")).await;

        let registration_status = get_registration_response(rx).await;

        assert!(
            registration_status.plugin_registered,
            "Plugin did not receive successful registration request"
        );

        assert!(
            registration_status.error.is_empty(),
            "Error message should be empty"
        );

        let plugin_endpoint = registrar
            .get_endpoint("foo")
            .await
            .expect("Should be able to get plugin info");
        assert_eq!(
            plugin_endpoint,
            PathBuf::from(FAKE_ENDPOINT),
            "Incorrect endpoint configured"
        );
    }

    #[tokio::test]
    async fn test_unsuccessful_registration() {
        let (tempdir, registrar) = setup();

        let (tx, rx) = mpsc::channel(1);

        let plugin = InvalidCsiPlugin {
            name: "foo".to_string(),
            registration_response: Mutex::new(tx),
        };

        start_registrar(registrar.clone()).await;

        setup_server(plugin, tempdir.path().join("foo.sock")).await;

        let registration_status = get_registration_response(rx).await;

        assert!(
            !registration_status.plugin_registered,
            "Plugin should not have been registered"
        );

        assert!(
            !registration_status.error.is_empty(),
            "Error message should be set"
        );

        assert!(
            registrar.get_endpoint("foo").await.is_none(),
            "Plugin shouldn't be registered in memory"
        );
    }

    #[tokio::test]
    async fn test_existing_socket() {
        let (tempdir, registrar) = setup();

        let (tx, rx) = mpsc::channel(1);

        let plugin = TestCsiPlugin {
            name: "foo".to_string(),
            registration_response: Mutex::new(tx),
        };

        // Make sure the plugin is running first so we can test that the registrar picks it up
        setup_server(plugin, tempdir.path().join("foo.sock")).await;

        // Delay to give it time to start
        tokio::time::sleep(Duration::from_secs(1)).await;

        start_registrar(registrar.clone()).await;

        let registration_status = get_registration_response(rx).await;

        assert!(
            registration_status.plugin_registered,
            "Plugin did not receive successful registration request"
        );

        assert!(
            registration_status.error.is_empty(),
            "Error message should be empty"
        );

        let plugin_endpoint = registrar
            .get_endpoint("foo")
            .await
            .expect("Should be able to get plugin info");
        assert_eq!(
            plugin_endpoint,
            PathBuf::from(FAKE_ENDPOINT),
            "Incorrect endpoint configured"
        );
    }

    #[tokio::test]
    async fn test_unregister() {
        let (tempdir, registrar) = setup();

        let (tx, rx) = mpsc::channel(1);

        let plugin = TestCsiPlugin {
            name: "foo".to_string(),
            registration_response: Mutex::new(tx),
        };

        // Manual server setup so we can kill the server
        let sock_path = tempdir.path().join("foo.sock");
        let socket = grpc_sock::server::Socket::new(&sock_path)
            .expect("unable to setup server listening on socket");

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let server = Server::builder()
                .add_service(RegistrationServer::new(plugin))
                .serve_with_incoming(socket);
            tokio::select! {
                res = server => {
                    res.expect("Unable to serve test plugin");
                    println!("server exited");
                }
                _ = stop_rx => {}
            }
        });

        // Delay to give it time to start
        tokio::time::sleep(Duration::from_secs(1)).await;

        start_registrar(registrar.clone()).await;

        let registration_status = get_registration_response(rx).await;

        assert!(
            registration_status.plugin_registered,
            "Plugin did not receive successful registration request"
        );

        // Now we can stop the plugin and delete the socket
        stop_tx.send(()).expect("Unable to send stop signal");

        tokio::fs::remove_file(sock_path)
            .await
            .expect("Unable to remove socket");

        // Delay to give it time to remove (needs to be a little longer for MacOS' sake)
        tokio::time::sleep(Duration::from_secs(3)).await;

        assert!(
            registrar.get_endpoint("foo").await.is_none(),
            "Plugin shouldn't be registered in memory"
        );
    }

    // This next section is all testing the validate function

    fn valid_info() -> PluginInfo {
        PluginInfo {
            r#type: "CSIPlugin".to_string(),
            name: "test".to_string(),
            endpoint: FAKE_ENDPOINT.to_string(),
            supported_versions: vec![API_VERSION.to_string()],
        }
    }

    #[tokio::test]
    async fn test_invalid_type() {
        // This path doesn't matter here
        let registrar = PluginRegistry::new("/tmp/foo");
        let mut info = valid_info();
        info.r#type = "DevicePlugin".to_string();

        assert!(
            registrar
                .validate(&info, &PathBuf::from("/fake"))
                .await
                .is_err(),
            "DevicePlugin type should error"
        );

        info.r#type = "NonExistent".to_string();
        assert!(
            registrar
                .validate(&info, &PathBuf::from("/fake"))
                .await
                .is_err(),
            "Invalid type should error"
        );
    }

    #[tokio::test]
    async fn test_invalid_plugin_version() {
        // This path doesn't matter here
        let registrar = PluginRegistry::new("/tmp/foo");
        let mut info = valid_info();
        info.supported_versions = vec!["v1beta1".to_string()];

        assert!(
            registrar
                .validate(&info, &PathBuf::from("/fake"))
                .await
                .is_err(),
            "Unsupported version should error"
        );
    }

    #[tokio::test]
    async fn test_invalid_name_with_different_endpoint() {
        // This path doesn't matter here
        let registrar = PluginRegistry::new("/tmp/foo");
        let mut info = valid_info();

        // Insert a valid registration
        let discovered_path = PathBuf::from("/tmp/foo/bar.sock");
        registrar.register(&info, &discovered_path).await;

        info.endpoint = "/another/path.sock".to_string();

        assert!(
            registrar.validate(&info, &discovered_path).await.is_err(),
            "Different endpoint with same name should error"
        );
    }

    #[tokio::test]
    async fn test_invalid_name_with_different_discovered_path() {
        // This path doesn't matter here
        let registrar = PluginRegistry::new("/tmp/foo");
        let mut info = valid_info();
        info.endpoint = String::new();

        // Insert a valid registration
        registrar
            .register(&info, &PathBuf::from("/tmp/foo/bar.sock"))
            .await;

        assert!(
            registrar
                .validate(&info, &PathBuf::from("/tmp/foo/another.sock"))
                .await
                .is_err(),
            "Different discovered path with same name should error"
        );
    }

    #[tokio::test]
    async fn test_reregistration() {
        // This path doesn't matter here
        let registrar = PluginRegistry::new("/tmp/foo");
        let info = valid_info();

        // Insert a valid registration
        let discovered_path = PathBuf::from("/tmp/foo/bar.sock");
        registrar.register(&info, &discovered_path).await;

        assert!(
            registrar.validate(&info, &discovered_path).await.is_ok(),
            "Exact same plugin info shouldn't fail"
        );
    }
}
