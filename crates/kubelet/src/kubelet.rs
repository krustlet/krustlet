///! This library contains code for running a kubelet. Use this to create a new
///! Kubelet with a specific handler (called a `Provider`)
use crate::config::Config;
use crate::node;
use crate::operator::PodOperator;
use crate::plugin_watcher::PluginRegistry;
use crate::provider::Provider;
use crate::webserver::start as start_webserver;

use futures::future::FutureExt;
use kube::api::ListParams;
use log::{error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal::ctrl_c;

use krator::OperatorContext;

/// A Kubelet server backed by a given `Provider`.
///
/// A Kubelet is a special kind of server that handles Kubernetes requests
/// to schedule pods.
///
/// The Kubelet creates a listener on the Kubernetes API (called an Informer),
/// a webserver for API callbacks, and a periodic updater to let Kubernetes
/// know that the node is still running.
///
/// The Provider supplies all of the backend-specific logic. Krustlet will only
/// run one (instance of a) Provider. So a provider may be passed around from
/// thread to thread during the course of the Kubelet's lifetime.
pub struct Kubelet<P> {
    provider: Arc<P>,
    kube_config: kube::Config,
    config: Box<Config>,
}

impl<P: Provider> Kubelet<P> {
    /// Create a new Kubelet with a provider, a kubernetes configuration,
    /// and a kubelet configuration
    pub async fn new(
        provider: P,
        kube_config: kube::Config,
        config: Config,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            provider: Arc::new(provider),
            kube_config,
            // The config object can get a little bit for some reason, so put it
            // on the heap
            config: Box::new(config),
        })
    }

    /// Begin answering requests for the Kubelet.
    ///
    /// This will listen on the given address, and will also begin watching for Pod
    /// events, which it will handle.
    pub async fn start(&self) -> anyhow::Result<()> {
        let client = kube::Client::new(self.kube_config.clone());

        // Create the node. If it already exists, this will exit
        node::create(&client, &self.config, self.provider.clone()).await;

        // Flag to indicate graceful shutdown has started.
        let signal = Arc::new(AtomicBool::new(false));
        let signal_task = start_signal_task(Arc::clone(&signal)).fuse().boxed();

        let plugin_registrar = PluginRegistry::new(&self.config.plugins_dir);

        let registrar = plugin_registrar.run().fuse().boxed();

        // Start the webserver
        let webserver = start_webserver(self.provider.clone(), &self.config.server_config)
            .fuse()
            .boxed();

        // Start updating the node lease and status periodically
        let node_updater = start_node_updater(client.clone(), self.config.node_name.clone())
            .fuse()
            .boxed();

        // If any of these tasks fail, we can initiate graceful shutdown.
        let services = Box::pin(async {
            tokio::select! {
                res = signal_task => if let Err(e) = res {
                    error!("Signal task completed with error {:?}", &e);
                },
                res = webserver => error!("Webserver task completed with result {:?}", &res),
                res = node_updater => if let Err(e) = res {
                    error!("Node updater task completed with error {:?}", &e);
                },
                res = registrar => if let Err(e) = res {
                    error!("Registrar task completed with error {:?}", &e);
                }
            };
            // Use relaxed ordering because we just need other tasks to eventually catch the signal.
            signal.store(true, Ordering::Relaxed);
            Ok::<(), anyhow::Error>(())
        });

        // Periodically checks for shutdown signal and cleans up resources gracefully if caught.
        let signal_handler = start_signal_handler(
            Arc::clone(&signal),
            client.clone(),
            self.config.node_name.clone(),
        )
        .fuse()
        .boxed();

        let operator = PodOperator::new(Arc::clone(&self.provider));
        let node_selector = format!("spec.nodeName={}", &self.config.node_name);
        let params = ListParams {
            field_selector: Some(node_selector),
            ..Default::default()
        };
        let mut operator_context = OperatorContext::new(&self.kube_config, operator, Some(params));
        let operator_task = operator_context.start().fuse().boxed();

        // These must all be running for graceful shutdown. An error here exits ungracefully.
        let core = Box::pin(async {
            tokio::select! {
                res = signal_handler => res.map_err(|e| {
                    error!("Signal handler task joined with error {:?}", &e);
                    e
                }),
                _ = operator_task => {
                    warn!("Pod operator has completed");
                    Ok(())
                }
            }
        });

        // Services will not return an error, so this will wait for both to return, or core to
        // return an error. Services will return if signal is set because pod_informer will drop
        // error_sender and error_handler will exit.
        tokio::try_join!(core, services)?;
        Ok(())
    }
}

// We cannot `#[derive(Clone)]` because that would place the
// unnecessary `P: Clone` constraint.
impl<P> Clone for Kubelet<P> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            kube_config: self.kube_config.clone(),
            config: self.config.clone(),
        }
    }
}

/// Awaits SIGINT and sets graceful shutdown flag if detected.
async fn start_signal_task(signal: Arc<AtomicBool>) -> anyhow::Result<()> {
    ctrl_c().await?;
    warn!("Caught keyboard interrupt.");
    signal.store(true, Ordering::Relaxed);
    Ok(())
}

/// Periodically renew node lease and status. Exits if signal is caught.
async fn start_node_updater(client: kube::Client, node_name: String) -> anyhow::Result<()> {
    let sleep_interval = std::time::Duration::from_secs(10);
    loop {
        node::update(&client, &node_name).await;
        tokio::time::delay_for(sleep_interval).await;
    }
}

/// Checks for shutdown signal and cleans up resources gracefully.
async fn start_signal_handler(
    signal: Arc<AtomicBool>,
    client: kube::Client,
    node_name: String,
) -> anyhow::Result<()> {
    let duration = std::time::Duration::from_millis(100);
    loop {
        if signal.load(Ordering::Relaxed) {
            info!("Signal caught.");
            node::drain(&client, &node_name).await?;
            break Ok(());
        }
        tokio::time::delay_for(duration).await;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::container::Container;
    use crate::pod::{Pod, Status};
    use crate::state::ResourceState;
    use k8s_openapi::api::core::v1::{
        Container as KubeContainer, EnvVar, EnvVarSource, ObjectFieldSelector, Pod as KubePod,
        PodSpec, PodStatus,
    };
    use kube::api::ObjectMeta;
    use std::collections::BTreeMap;
    use tokio::sync::RwLock;

    fn mock_client() -> kube::Client {
        kube::Client::new(kube::Config::new(
            reqwest::Url::parse("http://127.0.0.1:8080").unwrap(),
        ))
    }

    struct MockProvider;

    struct ProviderState;
    struct PodState;

    #[async_trait::async_trait]
    impl ResourceState for PodState {
        type Manifest = Pod;
        type Status = Status;
        type SharedState = ProviderState;
        async fn async_drop(self, _provider_state: &mut ProviderState) {}
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        type ProviderState = ProviderState;
        type InitialState = crate::pod::state::Stub;
        type TerminatedState = crate::pod::state::Stub;
        type PodState = PodState;

        const ARCH: &'static str = "mock";

        async fn initialize_pod_state(&self, _pod: &Pod) -> anyhow::Result<Self::PodState> {
            Ok(PodState)
        }

        fn provider_state(&self) -> crate::state::SharedState<ProviderState> {
            Arc::new(RwLock::new(ProviderState {}))
        }

        async fn logs(
            &self,
            _namespace: String,
            _pod: String,
            _container: String,
            _sender: crate::log::Sender,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_env_vars() {
        let container = Container::new(&KubeContainer {
            env: Some(vec![
                EnvVar {
                    name: "first".into(),
                    value: Some("value".into()),
                    ..Default::default()
                },
                EnvVar {
                    name: "second".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.labels.label".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "third".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.annotations.annotation".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "NAME".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.name".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "NAMESPACE".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "metadata.namespace".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "HOST_IP".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "status.hostIP".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                EnvVar {
                    name: "POD_IP".into(),
                    value_from: Some(EnvVarSource {
                        field_ref: Some(ObjectFieldSelector {
                            field_path: "status.podIP".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        });
        let name = "my-name".to_string();
        let namespace = Some("my-namespace".to_string());
        let mut labels = BTreeMap::new();
        labels.insert("label".to_string(), "value".to_string());
        let mut annotations = BTreeMap::new();
        annotations.insert("annotation".to_string(), "value".to_string());
        let pod = Pod::from(KubePod {
            metadata: ObjectMeta {
                labels: Some(labels),
                annotations: Some(annotations),
                name: Some(name),
                namespace,
                ..Default::default()
            },
            spec: Some(PodSpec {
                service_account_name: Some("svc".to_string()),
                ..Default::default()
            }),
            status: Some(PodStatus {
                host_ip: Some("10.21.77.1".to_string()),
                pod_ip: Some("10.21.77.2".to_string()),
                ..Default::default()
            }),
        });
        let env = MockProvider::env_vars(&container, &pod, &mock_client()).await;

        assert_eq!(
            "value",
            env.get("first").expect("key first should exist").as_str()
        );

        assert_eq!(
            "value",
            env.get("second").expect("metadata.labels.label").as_str()
        );
        assert_eq!(
            "value",
            env.get("third")
                .expect("metadata.annotations.annotation")
                .as_str()
        );
        assert_eq!("my-name", env.get("NAME").expect("metadata.name").as_str());
        assert_eq!(
            "my-namespace",
            env.get("NAMESPACE").expect("metadata.namespace").as_str()
        );
        assert_eq!("10.21.77.2", env.get("POD_IP").expect("pod_ip").as_str());
        assert_eq!("10.21.77.1", env.get("HOST_IP").expect("host_ip").as_str());
    }
}
