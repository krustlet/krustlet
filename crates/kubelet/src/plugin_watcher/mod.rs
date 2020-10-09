use crate::plugin_registration_api::v1::{
    registration_client::RegistrationClient, InfoRequest, PluginInfo, RegistrationStatus,
    API_VERSION,
};

use anyhow::Context;
use log::{debug, error, trace, warn};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher};
use tokio::fs::{create_dir_all, read_dir};
use tokio::stream::StreamExt;
use tokio::sync::{mpsc::unbounded_channel, RwLock};
use tonic::Request;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};

// TODO: Figure out the default for Windows
const DEFAULT_PLUGIN_PATH: &str = "/var/lib/kubelet/plugins_registry/";
const SOCKET_EXTENSION: &str = "sock";

/// An enum for capturing possible plugin types. This is purely for clarity and capturing this
/// information is a compiled type as the information we get from gRPC is a string
enum PluginType {
    CSIPlugin,
    DevicePlugin,
}

impl TryFrom<&str> for PluginType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "CSIPlugin" => Ok(PluginType::CSIPlugin),
            "DevicePlugin" => Ok(PluginType::DevicePlugin),
            _ => Err(anyhow::anyhow!(
                "Unknown plugin type {}. Allowed types are 'CSIPlugin' and 'DevicePlugin'",
                value
            )),
        }
    }
}

/// Internal storage structure for a plugin
struct PluginEntry {
    plugin_path: PathBuf,
    endpoint: Option<PathBuf>,
}

/// An internal storage plugin registry that implements most the same functionality as the [plugin
/// manager](https://github.com/kubernetes/kubernetes/tree/fd74333a971e2048b5fb2b692a9e043483d63fba/pkg/kubelet/pluginmanager)
/// in kubelet
pub struct PluginRegistrar {
    plugins: RwLock<HashMap<String, PluginEntry>>,
    plugin_dir: PathBuf,
}

impl Default for PluginRegistrar {
    fn default() -> Self {
        PluginRegistrar {
            plugin_dir: PathBuf::from(DEFAULT_PLUGIN_PATH),
            ..Default::default()
        }
    }
}

impl PluginRegistrar {
    /// Returns a new plugin registrar configured with the given plugin directory path
    pub fn new<P: AsRef<Path>>(plugin_dir: P) -> Self {
        PluginRegistrar {
            plugin_dir: PathBuf::from(plugin_dir.as_ref()),
            ..Default::default()
        }
    }

    /// Gets the endpoint for the given plugin name, returning `None` if it doesn't exist
    pub async fn get(&self, plugin_name: &str) -> Option<PathBuf> {
        let lock = self.plugins.read().await;
        lock.get(plugin_name)
            .map(|v| v.endpoint.as_ref().unwrap_or(&v.plugin_path).to_owned())
    }

    /// Starts the plugin registrar and runs all automatic plugin discovery and registration loops
    pub async fn run(&self) -> anyhow::Result<()> {
        let (stream_tx, mut stream_rx) = unbounded_channel::<NotifyResult<Event>>();
        let mut watcher: RecommendedWatcher = Watcher::new_immediate(move |res| {
            if let Err(e) = stream_tx.send(res) {
                error!("Unable to send inotify event into stream: {:?}", e)
            }
        })?;
        watcher.configure(Config::PreciseEvents(true))?;

        // Create plugin directory if it doesn't exist
        create_dir_all(&self.plugin_dir).await?;

        // Walk the plugin dir before hand and process any currently existing sockets
        let dir_entries: Vec<PathBuf> = read_dir(&self.plugin_dir)
            .await?
            .map(|res| res.map(|entry| entry.path()))
            .collect::<Result<Vec<PathBuf>, _>>()
            .await?;

        // Manually assemble an event and call handle_event
        self.handle_create(Event {
            paths: dir_entries,
            ..Default::default()
        })
        .await?;

        watcher.watch(&self.plugin_dir, RecursiveMode::NonRecursive)?;

        while let Some(res) = stream_rx.recv().await {
            match res {
                Ok(event) if event.kind.is_create() => {
                    if let Err(e) = self.handle_create(event).await {
                        error!("An error occurred while processing a new plugin: {:?}", e);
                    }
                }
                Ok(event) if event.kind.is_remove() => self.handle_delete(event).await,
                // Skip any events that aren't create or delete
                Ok(_) => continue,
                Err(e) => error!("An error occurred while watching the plugin directory. Will continue to retry: {:?}", e),
            }
        }
        Ok(())
    }

    async fn handle_create(&self, event: Event) -> anyhow::Result<()> {
        // Filter paths, checking if it is a socket and file
        for discovered_path in event
            .paths
            .into_iter()
            .filter(|p| p.is_file() && p.extension().unwrap_or_default() == SOCKET_EXTENSION)
        {
            debug!(
                "Beginning plugin registration for plugin discovered at {}",
                discovered_path.display()
            );

            // Step 1: Attempt to call the socket. If this fails, we don't have any guarantee we'll
            // be able to inform it we've failed. So just unwrap the error here
            let plugin_info = get_plugin_info(&discovered_path).await?;
            debug!(
                "Successfully retrieved information for plugin discovered at {}:\n {:?}",
                discovered_path.display(),
                plugin_info
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
                "Successfully validated plugin discovered at {}:\n {:?}",
                discovered_path.display(),
                plugin_info
            );

            // Step 3: Register plugin to local storage
            self.register(&plugin_info, &discovered_path).await;

            // Step 4: Inform plugin
            inform_plugin(&discovered_path, None).await?;
            debug!("Plugin registration complete for {:?}", plugin_info)
        }
        Ok(())
    }

    async fn handle_delete(&self, event: Event) {
        let mut lock = self.plugins.write().await;
        for deleted_plugin in event
            .paths
            .into_iter()
            .filter(|p| p.is_file() && p.extension().unwrap_or_default() == SOCKET_EXTENSION)
        {
            let key = match lock.iter().find(|(_, v)| *v.plugin_path == deleted_plugin) {
                // Take ownership of the key to avoid an immutable borrow
                Some((key, _)) => key.to_owned(),
                // If for some reason it is already gone, no need to error
                None => continue,
            };
            lock.remove(&key);
        }
    }

    /// Validates the given plugin info gathered from a discovered plugin, returning an error with
    /// additional information if it is not valid. This will validate 3 specific things (should
    /// answer YES to all of these):
    /// 1. Is it a CSIPlugin? If it isn't we will deny it for now. This will be removed as we
    ///    iterate and if it is needed
    /// 2. Does the list of supported versions contain the version we expect?
    /// 3. Is the plugin name available? 3a. If the name is already registered, is the endpoint the
    ///    exact same? If it is, we allow it to reregister
    async fn validate(&self, info: &PluginInfo, discovered_path: &PathBuf) -> anyhow::Result<()> {
        trace!(
            "Starting validation for plugin {:?} discovered at path {}",
            info,
            discovered_path.display()
        );
        // Step 1: Check for valid type and if it is a CSIPlugin
        let plugin_type = PluginType::try_from(info.r#type.as_str())?;
        trace!("Type validation complete for plugin {:?}", info);
        if matches!(plugin_type, PluginType::DevicePlugin) {
            warn!("DevicePlugins are not currently supported");
            return Err(anyhow::anyhow!("DevicePlugins are not currently supported"));
        }

        // Step 2: Check if we support this version
        trace!("Checking supported versions for plugin {:?}", info);
        if !info.supported_versions.iter().any(|s| s == API_VERSION) {
            return Err(anyhow::anyhow!(
                "Plugin doesn't support version {}",
                API_VERSION
            ));
        }
        trace!("Supported version check complete for plugin {:?}", info);

        // Step 3: Check if it exists and if the path is different
        trace!("Checking for naming collisions for plugin {:?}", info);
        let lock = self.plugins.read().await;

        if let Some(current_path) = lock.get(&info.name) {
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
        trace!("Naming collision check complete for plugin {:?}", info);

        Ok(())
    }

    /// Registers the plugin in our HashMap
    async fn register(&self, info: &PluginInfo, discovered_path: &PathBuf) {
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
}

/// Attempts a `GetInfo` gRPC call to the endpoint to the path given
async fn get_plugin_info(path: &PathBuf) -> anyhow::Result<PluginInfo> {
    let connection_str = format!("unix://{}", path.display());
    trace!("Connecting to plugin at {} for GetInfo", connection_str);
    let mut client = RegistrationClient::connect(connection_str.clone()).await?;

    let req = Request::new(InfoRequest {});

    trace!("Calling GetInfo at {}", connection_str);
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
async fn inform_plugin(path: &PathBuf, error: Option<String>) -> anyhow::Result<()> {
    let connection_str = format!("unix://{}", path.display());
    trace!(
        "Connecting to plugin at {} for NotifyRegistrationStatus",
        connection_str
    );
    let mut client = RegistrationClient::connect(connection_str.clone()).await?;

    let req = Request::new(RegistrationStatus {
        plugin_registered: error.is_none(),
        error: error.unwrap_or_else(String::new),
    });

    trace!("Calling NotifyRegistrationStatus at {}", connection_str);
    client
        .notify_registration_status(req)
        .await
        .map(|_| ())
        .map_err(|status| {
            anyhow::anyhow!(
                "NotifyRegistrationStatus call to {} failed with error code {} and message {}",
                path.display(),
                status.code(),
                status.message()
            )
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::plugin_registration_api::v1::{
        registration_server::{Registration, RegistrationServer}, InfoRequest, PluginInfo, RegistrationStatusResponse, RegistrationStatusResponse,
        API_VERSION,
    };
    use tonic::{transport::Server, Request, Response, Status, Code};
    use tokio::sync::mpsc::{self, Sender};

    #[derive(Debug, Default)]
    struct PluginConfig {
        get_info_error: Option<String>,
        registration_notify_error: Option<String>
    }

    #[derive(Debug, Default)]
    struct TestPlugin {
        conf: PluginConfig,
        plugin_type: String,
        name: String,
        endpoint: Option<String>,
        supported_versions: Vec<String>,
        // A channel to send out the response it gets from registering
        registration_response: Sender<RegistrationStatus>,
    }

    #[tonic::async_trait]
    impl Registration for TestPlugin {
        async fn get_info(&self, req: Request<InfoRequest>) -> Result<Response<PluginInfo>, Status> {
            if let Some(e) = self.conf.get_info_error.as_ref() {
                return Err(Status::new(Code::Unavailable, e))
            }
            Ok(Response::new(PluginInfo {
                r#type: self.plugin_type.clone(),
                name: self.name.clone(),
                endpoint: self.endpoint.clone().unwrap_or_default(),
                supported_versions: self.supported_versions.clone()
            }))
        }

        async fn notify_registration_status(&self, req: Request<RegistrationStatus>) -> Result<Response<RegistrationStatusResponse>, Status> {
            if let Some(e) = self.conf.registration_notify_error.as_ref() {
                return Err(Status::new(Code::Unavailable, e))
            }

            self.registration_response.send(req.into_inner()).expect("should be able to send registration status on channel");

            Ok(Response::new(RegistrationStatusResponse{}))
        }
    }

    /// Setup the test, returning the temporary directory (as we need it around to keep it from
    /// dropping) and a PluginRegistrar configured with the path
    fn setup() -> (tempfile::TempDir, PluginRegistrar) {
        let tempdir = tempfile::tempdir().expect("should be able to create tempdir");

        let registrar = PluginRegistrar::new(&tempdir);

        (tempdir, registrar)
    }

    #[tokio::test]
    async fn test_successful_registration() {
        let (tempdir, registrar) = setup();

        let (tx, rx) = mpsc::channel(1);

        let plugin = TestPlugin {
            plugin_type: "CSIPlugin",
            name: "foo",
            endpoint: Some("/tmp/foo.sock"),
            supported_versions: vec![API_VERSION.to_string()],
            registration_response: tx,
            ..Default::default()
        };

        let mut sock_path = tempdir.path().to_path_buf();
        sock_path.push("foo.sock");

        let addr = sock_path.to_str().unwrap_or("").parse().expect("expected socket path to parse");

        tokio::spawn(Server::builder().add_service(RegistrationServer::new(plugin)).serve(addr));

        let registration_status = rx.recv().await.expect("Should have received a valid response in the channel");

        assert!(registration_status.plugin_registered, "Plugin did not receive successful registration request");

        assert!(registration_status.error.is_empty(), "Error message should be empty");
    }

    #[tokio::test]
    async fn test_unsuccessful_registration() {}

    #[tokio::test]
    async fn test_validation() {}

    #[tokio::test]
    async fn test_existing_socket() {}

    #[tokio::test]
    async fn test_unregister() {}
}
