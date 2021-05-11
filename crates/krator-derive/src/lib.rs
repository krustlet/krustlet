//! A crate for deriving state machine traits in Kubelet.
//!
//! Right now this crate consists of a [TransitionTo] derive macro for the `TransitionTo` trait.
//!
//! In addition to the `derive` attribute, this macro
//! also requires the use of a custom attribute called `transition_to` that specifies the types that
//! can be transitioned to. Not specifying this attribute will result in a compile time error.
//!
//! If the feature `admission-webhook` is enabled, this crate provides a [AdmissionWebhook] derive macro that
//! provides functions for creating necessary resources for running a admission webhook.
extern crate proc_macro;
use crate::proc_macro::TokenStream;
mod transitions;

#[proc_macro_derive(TransitionTo, attributes(transition_to))]
pub fn derive_transition_to(input: TokenStream) -> TokenStream {
    transitions::run_custom_derive(input)
}

#[cfg(feature = "admission-webhook")]
mod admission;

#[cfg(feature = "admission-webhook")]
#[proc_macro_derive(AdmissionWebhook, attributes(admission_webhook_features))]
///
/// Provides functions for creating necessary resources for running a admission webhook. As this derive macro can create code that needs
/// additional dependencies, it needs to be configured with the `admission_webhook_features` attribute (see example below).
///
/// ## Features and their respective functions and necessary dependencies:
///
/// ### `#[admission_webhook_features(service)]`
///
/// provides the following functions on your struct:
///
///   - `admission_webhook_service_app_selector() -> String`: Returns the selector value for the label app that the service created with
///      `admission_webhook_service()` uses for selecting the pods that serve the admission webhook
///   - `admission_webhook_service_name() -> String`:  Returns the name of the admission webhook service that will get
///      created with `admission_webhook_service()`
///   - `admission_webhook_service(namespace: &str) -> Service`: Returns a service that forwards to pods where label `app`
///      has the value returned by the function `admission_webhook_service_app_selector()`. It exposes port `443`
///      (necessary for webhooks) and listens to the pod's port `8443`
///
///  requires the following crate: **`k8s_openapi`**
///
/// ### `#[admission_webhook_features(secret)]`
///
/// provides the following functions on your struct:
///
///   - `admission_webhook_secret_name() -> String`: Returns the name of the admission webhook secret that will get created with `admission_webhook_secret()`
///   - `admission_webhook_secret(namespace: &str) -> Secret`: Creates a Kubernetes secret of type `tls` that contains a
///     certificate and a private key and can be used for the admission webhook service
///
///  requires the following crate: **`k8s_openapi`**, **`rcgen` (with `features = ["x509-parser", "pem"]`)**
///
///
/// ### `#[admission_webhook_features(admission_webhook_config)]`
///
/// provides the following functions on your struct:
///
/// - `admission_webhook_resources(namespace: &str) -> (Service, Secret, MutatingWebhookConfiguration)`: Convenience function that returns all Kubernetes
///   resources necessary to configure an admission webhook service. It expects a deployed pod with label app having the value returned by admission_webhook_service_name().
/// - `admission_webhook_configuration_name() -> String`:Returns the name of the admission webhook configuration that will get
///    created with `admission_webhook_configuration()`
///
///  requires the following crate: **`k8s_openapi`**
///
/// ### `#[admission_webhook_features(secret,service,admission_webhook_config)]`
///
/// if you activate all features, all the functions above will be provided and additionally
/// this convenience function
///
/// - `admission_webhook_configuration(service: Service, secret: Secret) -> Result<MutatingWebhookConfiguration>`:  Creates a MutatingWebhookConfiguration using
///    the certificate from the given service and the service of the given service as configuration
///
///
/// Example
/// ```
/// pub use schemars::JsonSchema;
/// pub use krator_derive::AdmissionWebhook;
/// pub use kube::CustomResource;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(
///    AdmissionWebhook,
///    CustomResource,
///    Serialize,
///    Deserialize,
///    PartialEq,
///    Default,
///    Debug,
///    Clone,
///    JsonSchema,
/// )]
/// #[admission_webhook_features(secret,service,admission_webhook_config)]
/// #[kube(group = "example.com", version = "v1", kind = "MyCr")]
/// pub struct CrSpec {
///     pub name: String,
/// }
///
/// let (service, secret, admission_webhook_configuration) =
///         MyCr::admission_webhook_resources("default");
///
///
/// assert_eq!(
///     MyCr::admission_webhook_configuration_name(),
///     "mycrs.example.com".to_string()
/// );
///
/// assert_eq!(
///     admission_webhook_configuration.metadata.name.unwrap(),
///     MyCr::admission_webhook_configuration_name(),
/// );
///
/// assert_eq!(
///     MyCr::admission_webhook_service_name(),
///     "mycrs-example-com-admission-webhook".to_string()
/// );
///
/// assert_eq!(
///     service.metadata.name.unwrap(),
///     MyCr::admission_webhook_service_name(),
/// );
///
/// assert_eq!(
///     MyCr::admission_webhook_secret_name(),
///     "mycrs-example-com-admission-webhook-tls".to_string()
/// );
///
/// assert_eq!(
///     secret.metadata.name.unwrap(),
///     MyCr::admission_webhook_secret_name(),
/// );
/// ```
pub fn derive_admission_webhook(input: TokenStream) -> TokenStream {
    admission::run_custom_derive::<admission::CustomResourceInfos>(input)
}
