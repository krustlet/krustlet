//! Basic implementation of Kubernetes Admission API
use crate::Operator;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
use kube::api::Meta;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, trace, warn};
use tracing_futures::Instrument;

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

impl<T: Meta> AdmissionRequest<T> {
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
    let cert_path = std::env::var("ADMISSION_CERT_PATH").expect("No certificate path specified.");
    let key_path = std::env::var("ADMISSION_KEY_PATH").expect("No key path specified.");
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
        .cert_path(&cert_path)
        .key_path(&key_path)
        .run(([0, 0, 0, 0], 8443))
        .await;
}
