//! Basic implementation of Kubernetes Admission API
use crate::Operator;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Result of admission hook.
#[allow(clippy::large_enum_variant)]
pub enum AdmissionResult<T> {
    /// Permit the request. Pass the object (with possible mutations) back.
    /// JSON Patch of any changes will automatically be created.
    Allow(T),
    /// Deny the request. Pass a Status object to provide information about the error.
    Deny(Status),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
/// AdmissionRequest describes the admission.Attributes for the admission request.
struct AdmissionRequest<T> {
    /// UID is an identifier for the individual request/response. It allows us to distinguish instances of requests which are
    /// otherwise identical (parallel requests, requests when earlier requests did not modify etc)
    /// The UID is meant to track the round trip (request/response) between the KAS and the WebHook, not the user request.
    /// It is suitable for correlating log entries between the webhook and apiserver, for either auditing or debugging.
    uid: Option<String>,
    /// Object is the object from the incoming request.
    object: T,
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
                let original = serde_json::to_value(&request.request.object).unwrap();
                let response = match operator.admission_hook(request.request.object).await {
                    AdmissionResult::Allow(manifest) => {
                        let value = serde_json::to_value(&manifest).unwrap();
                        let patch = json_patch::diff(&original, &value);
                        let (patch, patch_type) = if !patch.0.is_empty() {
                            (Some(patch), Some("JSONPatch".to_string()))
                        } else {
                            (None, None)
                        };
                        AdmissionResponse {
                            uid: request.request.uid,
                            allowed: true,
                            status: None,
                            patch,
                            patch_type,
                        }
                    }
                    AdmissionResult::Deny(status) => AdmissionResponse {
                        uid: request.request.uid,
                        allowed: false,
                        status: Some(status),
                        patch: None,
                        patch_type: None,
                    },
                };
                Ok::<_, std::convert::Infallible>(warp::reply::json(&AdmissionReviewResponse {
                    api_version: request.api_version,
                    kind: request.kind,
                    response,
                }))
            }
        });
    warp::serve(routes)
        .tls()
        .cert_path(&cert_path)
        .key_path(&key_path)
        .run(([0, 0, 0, 0], 8443))
        .await;
}
