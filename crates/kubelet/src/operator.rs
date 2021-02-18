use crate::pod::initialize_pod_container_statuses;
use crate::pod::Pod;
use crate::provider::Provider;
use crate::volume::Ref;
use k8s_openapi::api::core::v1::Pod as KubePod;
use krator::state::SharedState;
use krator::ObjectState;
use krator::{Manifest, Operator};
use kube::Api;
use log::error;
use std::sync::Arc;

pub(crate) struct PodOperator<P: Provider> {
    provider: Arc<P>,
    client: kube::Client,
}

impl<P: Provider> PodOperator<P> {
    pub fn new(provider: Arc<P>, client: kube::Client) -> Self {
        PodOperator { provider, client }
    }
}

#[async_trait::async_trait]
impl<P: Provider> Operator for PodOperator<P> {
    type Manifest = crate::pod::Pod;
    type Status = crate::pod::Status;
    type ObjectState = P::PodState;
    type InitialState = P::InitialState;
    type DeletedState = P::TerminatedState;

    async fn initialize_object_state(&self, manifest: &Pod) -> anyhow::Result<P::PodState> {
        self.provider.initialize_pod_state(manifest).await
    }

    async fn shared_state(&self) -> SharedState<<P::PodState as ObjectState>::SharedState> {
        self.provider.provider_state()
    }

    async fn registration_hook(&self, manifest: Manifest<Self::Manifest>) -> anyhow::Result<()> {
        let initial_manifest = manifest.latest();
        let namespace = initial_manifest.namespace();
        let name = initial_manifest.name().to_string();
        let api: Api<KubePod> = Api::namespaced(self.client.clone(), namespace);

        initialize_pod_container_statuses(name, manifest, &api).await
    }

    async fn deregistration_hook(&self, manifest: Manifest<Self::Manifest>) -> anyhow::Result<()> {
        if let Some(volume_path) = self.provider.volume_path() {
            let pod = manifest.latest();
            let plugin_registry = self.provider.plugin_registry();
            match Ref::unmount_volumes_from_pod(&volume_path, &pod, &self.client, plugin_registry)
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    // report the error, but carry on. Volume unmount failures should not result in a panic()
                    error!("{:?}", e);
                }
            };
        }
        Ok(())
    }
}
