//! Basic implementation of Kubernetes Admission API
use crate::Operator;
use anyhow::{bail, ensure, Context};
use k8s_openapi::{
    api::{
        admissionregistration::v1::MutatingWebhookConfiguration,
        core::v1::{Secret, Service},
    },
    apimachinery::pkg::apis::meta::v1::Status,
};
use k8s_openapi::{apimachinery::pkg::apis::meta::v1::OwnerReference, Metadata};
use kube::{
    api::{ObjectMeta, Patch, PatchParams},
    Client, Resource,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Display, Formatter},
    sync::Arc,
};
use tracing::{info, trace, warn};
use tracing_futures::Instrument;

/// WebhookResources encapsulates Kubernetes resources necessary to register the admission webhook.
/// and provides some convenience functions
///
/// # Examples
/// ```
/// use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
/// use krator::admission;
/// use krator_derive::AdmissionWebhook;
/// use kube::{Api, Client};
/// use kube_derive::CustomResource;
/// use schemars::JsonSchema;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(
///   AdmissionWebhook,
///   CustomResource,
///   Serialize,
///   Deserialize,
///   PartialEq,
///   Default,
///   Debug,
///   Clone,
///   JsonSchema,
/// )]
/// #[admission_webhook_features(secret, service, admission_webhook_config)]
/// #[kube(
/// group = "example.com",
/// version = "v1",
/// kind = "MyCr",
/// )]
/// pub struct MyCrSpec {
///     pub owner: String,
/// }
///
/// async fn install_webhook_resources() -> anyhow::Result<()> {
///   let client = Client::try_default().await?;
///   let namespace = "default";

///   let webhook_resources =
///       admission::WebhookResources::from(MyCr::admission_webhook_resources(namespace));
///
///   println!("{}", webhook_resources); // print resources as yaml
///
///   // get the installed crd resource
///   let my_crd = Api::<CustomResourceDefinition>::all(client.clone())
///       .get(&MyCr::crd().metadata.name.unwrap())
///       .await
///       .unwrap();
///
///   // install the necessary resources for serving a admission controller (service, secret, mutatingwebhookconfig)
///   // and make them owned by the crd ... this way, they will all be deleted once the crd gets deleted
///   webhook_resources
///       .apply_owned(&client, &my_crd)
///       .await
/// }
/// ```
///
pub struct WebhookResources(pub Service, pub Secret, pub MutatingWebhookConfiguration);

impl From<(Service, Secret, MutatingWebhookConfiguration)> for WebhookResources {
    fn from(tuple: (Service, Secret, MutatingWebhookConfiguration)) -> Self {
        WebhookResources(tuple.0, tuple.1, tuple.2)
    }
}

impl WebhookResources {
    /// returns the service
    pub fn service(&self) -> &Service {
        &self.0
    }

    /// returns the secret
    pub fn secret(&self) -> &Secret {
        &self.1
    }

    /// returns the webhook_config
    pub fn webhook_config(&self) -> &MutatingWebhookConfiguration {
        &self.2
    }
    /// adds an owner to the webhook resources
    pub fn add_owner<T>(&self, owner: &T) -> Self
    where
        T: Resource + Metadata<Ty = ObjectMeta>,
    {
        let metadata = owner.metadata();

        let owner_references = Some(vec![OwnerReference {
            api_version: k8s_openapi::api_version(owner).to_string(),
            controller: Some(true),
            kind: k8s_openapi::kind(owner).to_string(),
            name: metadata.name.clone().unwrap(),
            uid: metadata.uid.clone().unwrap(),
            ..Default::default()
        }]);

        let mut secret = self.secret().to_owned();
        secret.metadata.owner_references = owner_references.clone();

        let mut service = self.service().to_owned();
        service.metadata.owner_references = owner_references.clone();

        let mut webhook_config = self.webhook_config().to_owned();
        webhook_config.metadata.owner_references = owner_references;

        WebhookResources(service, secret, webhook_config)
    }

    /// applies the webhook resources and makes them owned by the object
    /// that the `owner` resource belongs to -- this way the resource will get deleted
    /// automatically, when the owner gets deleted.
    ///
    /// it will create the resources if they
    /// are not present yet or replace them if they already exist
    ///
    /// due to the necessary permissions (create/update permission for secrets, services and admission-config),
    /// this should not automatically be executed when the operator starts
    pub async fn apply_owned<T>(&self, client: &Client, owner: &T) -> anyhow::Result<()>
    where
        T: Resource + Metadata<Ty = ObjectMeta>,
    {
        self.add_owner(owner).apply(client).await
    }

    /// applies the webhook resources to the cluster, i.e. it will create the resources if they
    /// are not present yet or replace them if they already exist
    ///
    /// due to the necessary permissions (create/update permission for secrets, services and admission-config),
    /// this should not automatically be executed when the operator starts
    pub async fn apply(&self, client: &Client) -> anyhow::Result<()> {
        let secret_namespace = self.secret().metadata.namespace.as_ref().with_context(|| {
            format!(
                "secret {} does not have namespace set",
                self.secret()
                    .metadata
                    .name
                    .clone()
                    .unwrap_or_else(|| "".to_string())
            )
        })?;
        let service_namespace = self
            .service()
            .metadata
            .namespace
            .as_ref()
            .with_context(|| {
                format!(
                    "service {} does not have namespace set",
                    self.service()
                        .metadata
                        .name
                        .as_ref()
                        .unwrap_or(&"".to_string())
                )
            })?;

        {
            let api: kube::Api<Secret> = kube::Api::namespaced(client.to_owned(), secret_namespace);
            let name = self.secret().metadata.name.as_ref().unwrap();
            api.patch(
                &name,
                &PatchParams {
                    dry_run: false,
                    force: true,
                    field_manager: Some("krator".to_string()),
                },
                &Patch::Apply(self.secret()),
            )
            .await?;
        }

        {
            let api: kube::Api<Service> =
                kube::Api::namespaced(client.to_owned(), service_namespace);
            let name = self.service().metadata.name.as_ref().unwrap();
            api.patch(
                &name,
                &PatchParams {
                    dry_run: false,
                    force: true,
                    field_manager: Some("krator".to_string()),
                },
                &Patch::Apply(self.service()),
            )
            .await?;
        }

        {
            let api: kube::Api<MutatingWebhookConfiguration> = kube::Api::all(client.to_owned());
            let name = self.webhook_config().metadata.name.as_ref().unwrap();
            api.patch(
                &name,
                &PatchParams {
                    dry_run: false,
                    force: true,
                    field_manager: Some("krator".to_string()),
                },
                &Patch::Apply(self.webhook_config()),
            )
            .await?;
        }

        Ok(())
    }
}

impl Display for WebhookResources {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let service = self.service();
        write!(
            f,
            r#"
# resources necessary to expose the operator's webhook
# the service expects a pod with the labels
#
#    {:?}
#
# in namespace {}
#
# the service for the webhook
{}

# the secret containing the certificate and the private key the
# webhook service uses for secure communication
{}

# the webhook configuration
{}
"#,
            service.spec.clone().unwrap().selector.unwrap(),
            service.metadata.namespace.as_ref().unwrap(),
            serde_yaml::to_string(self.service()).unwrap(),
            serde_yaml::to_string(self.secret()).unwrap(),
            serde_yaml::to_string(self.webhook_config()).unwrap()
        )
    }
}

/// AdmissionTls wraps certificate and private key for the admission webhook server. If you read
/// the secret from a Kubernets secret, use the convenience function [AdmissionTls::from()]
pub struct AdmissionTls {
    /// tls certificate
    pub cert: String,
    /// tls private key
    pub private_key: String,
}

impl AdmissionTls {
    /// Convenience function to extract secret data from a Kubernetes secret of type `tls`. It supports
    /// Secrets that have secrets set via `data` or `string_data`
    pub fn from(s: &Secret) -> anyhow::Result<Self> {
        ensure!(
            s.type_.as_ref().unwrap() == "tls",
            "only tls secrets can be converted to AdmisstionTLS struct"
        );

        let metadata = &s.metadata;
        let error_msg = |key: &str| {
            format!(
                "secret data {}/{} does not contain key {}",
                metadata.name.as_ref().unwrap_or(&"".to_string()),
                metadata.namespace.as_ref().unwrap_or(&"".to_string()),
                key
            )
        };

        const TLS_CRT: &str = "tls.crt";
        const TLS_KEY: &str = "tls.key";

        if let Some(data) = &s.data {
            let cert_byte_string = data.get(TLS_CRT).context(error_msg(TLS_CRT))?;
            let key_byte_string = data.get(TLS_KEY).context(error_msg(TLS_KEY))?;

            return Ok(AdmissionTls {
                cert: std::str::from_utf8(&cert_byte_string.0)?.to_string(),
                private_key: std::str::from_utf8(&key_byte_string.0)?.to_string(),
            });
        }

        if let Some(string_data) = &s.string_data {
            let cert = string_data.get(TLS_CRT).context(error_msg(TLS_CRT))?;
            let key = string_data.get(TLS_KEY).context(error_msg(TLS_KEY))?;

            return Ok(AdmissionTls {
                cert: cert.to_string(),
                private_key: key.to_string(),
            });
        }

        bail!(
            "secret {}/{} does not contain any data",
            metadata.name.as_ref().unwrap_or(&"".to_string()),
            metadata.namespace.as_ref().unwrap_or(&"".to_string())
        )
    }
}

/// Result of admission hook.
#[allow(clippy::large_enum_variant)]
pub enum AdmissionResult<T> {
    /// Permit the request. Pass the object (with possible mutations) back.
    /// JSON Patch of any changes will automatically be created.
    Allow(T),
    /// Deny the request. Pass a Status object to provide information about the error.
    Deny(Status),
}

#[derive(Debug)]
enum Operation {
    Create,
    Update,
    Delete,
}

#[derive(Deserialize, Debug)]
struct UserInfo {
    username: String,
    groups: Vec<String>,
}

#[derive(Deserialize)]
#[serde(tag = "operation")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// `object` is the object from the incoming request.
/// `old_object` is the existing object. Only populated for DELETE and UPDATE requests.
enum AdmissionRequestOperation<T> {
    Create {
        object: T,
    },
    Update {
        object: T,
        #[serde(rename = "oldObject")]
        old_object: T,
    },
    Delete {
        #[serde(rename = "oldObject")]
        old_object: T,
    },
}

/// AdmissionRequest describes the admission.Attributes for the admission request.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdmissionRequest<T> {
    /// Identifier for the individual request/response.
    uid: Option<String>,
    /// Information about the requesting user.
    user_info: UserInfo,
    #[serde(flatten)]
    operation: AdmissionRequestOperation<T>,
}

impl<T: Resource> AdmissionRequest<T> {
    fn name(&self) -> String {
        match &self.operation {
            AdmissionRequestOperation::Create { object, .. } => object.name(),
            AdmissionRequestOperation::Update { object, .. } => object.name(),
            AdmissionRequestOperation::Delete { old_object, .. } => old_object.name(),
        }
    }

    fn namespace(&self) -> Option<String> {
        match &self.operation {
            AdmissionRequestOperation::Create { object, .. } => object.namespace(),
            AdmissionRequestOperation::Update { object, .. } => object.namespace(),
            AdmissionRequestOperation::Delete { old_object, .. } => old_object.namespace(),
        }
    }

    fn operation(&self) -> Operation {
        match &self.operation {
            AdmissionRequestOperation::Create { .. } => Operation::Create,
            AdmissionRequestOperation::Update { .. } => Operation::Update,
            AdmissionRequestOperation::Delete { .. } => Operation::Delete,
        }
    }
}

/// AdmissionResponse describes an admission response.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdmissionResponse {
    /// UID is an identifier for the individual request/response.
    /// This must be copied over from the corresponding AdmissionRequest.
    uid: Option<String>,
    /// Allowed indicates whether or not the admission request was permitted.
    allowed: bool,
    /// Result contains extra details into why an admission request was denied.
    /// This field IS NOT consulted in any way if "Allowed" is "true".
    status: Option<Status>,
    /// The patch body. Currently we only support "JSONPatch" which implements RFC 6902.
    patch: Option<json_patch::Patch>,
    /// The type of Patch. Currently we only allow "JSONPatch".
    patch_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdmissionReviewRequest<T> {
    api_version: String,
    kind: String,
    request: AdmissionRequest<T>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdmissionReviewResponse {
    api_version: String,
    kind: String,
    response: AdmissionResponse,
}

#[tracing::instrument(
    level="debug",
    skip(operator, request),
    fields(
        name=%request.request.name(),
        namespace=?request.request.namespace(),
        api_version=%request.api_version,
        operation=?request.request.operation(),
        user_info=?request.request.user_info
    )
)]
async fn review<O: Operator>(
    operator: Arc<O>,
    request: AdmissionReviewRequest<O::Manifest>,
) -> warp::reply::Json {
    let manifest = match request.request.operation {
        AdmissionRequestOperation::Create { object, .. } => object,
        AdmissionRequestOperation::Update {
            old_object, object, ..
        } => {
            let value = serde_json::to_value(&object).unwrap();
            let old_value = serde_json::to_value(&old_object).unwrap();
            let diff = json_patch::diff(&old_value, &value);
            if !diff.0.is_empty() {
                trace!(
                    diff=%format!("{:#?}", diff),
                    "Object changed."
                );
            }
            object
        }
        AdmissionRequestOperation::Delete { old_object, .. } => old_object,
    };

    let name = manifest.name();
    let namespace = manifest.namespace();

    let span = tracing::debug_span!("Operator::admission_hook",);

    let result = operator
        .admission_hook(manifest.clone())
        .instrument(span)
        .await;

    let response = match result {
        AdmissionResult::Allow(new_manifest) => {
            let new_value = serde_json::to_value(&new_manifest).unwrap();
            let old_value = serde_json::to_value(&manifest).unwrap();

            let patch = json_patch::diff(&old_value, &new_value);
            let (patch, patch_type) = if !patch.0.is_empty() {
                (Some(patch), Some("JSONPatch".to_string()))
            } else {
                (None, None)
            };
            info!(
                %name,
                ?namespace,
                allowed=true,
                ?patch,
                "Admission request allowed."
            );
            AdmissionResponse {
                uid: request.request.uid,
                allowed: true,
                status: None,
                patch,
                patch_type,
            }
        }
        AdmissionResult::Deny(status) => {
            warn!(
                code=?status.code,
                reason=?status.reason,
                message=?status.message,
                %name,
                ?namespace,
                allowed=false,
                "Admission request denied."
            );
            AdmissionResponse {
                uid: request.request.uid,
                allowed: false,
                status: Some(status),
                patch: None,
                patch_type: None,
            }
        }
    };
    warp::reply::json(&AdmissionReviewResponse {
        api_version: request.api_version,
        kind: request.kind,
        response,
    })
}

pub(crate) async fn endpoint<O: Operator>(operator: Arc<O>) {
    let tls = operator
        .admission_hook_tls()
        .await
        .expect("getting webhook tls AdmissionTls failed");

    use warp::Filter;
    let routes = warp::any()
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |request: AdmissionReviewRequest<O::Manifest>| {
            let operator = Arc::clone(&operator);
            async move {
                let response = review(operator, request).await;
                Ok::<_, std::convert::Infallible>(response)
            }
        });

    warp::serve(routes)
        .tls()
        .cert(tls.cert)
        .key(tls.private_key)
        .run(([0, 0, 0, 0], 8443))
        .await;
}
