use k8s_openapi::api::admissionregistration::v1::MutatingWebhookConfiguration;
use krator_derive::AdmissionWebhook;
use kube::CustomResource;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::PartialEq;

// TODO: follow up on https://github.com/clux/kube-rs/issues/264#issuecomment-748327959
#[derive(
    AdmissionWebhook,
    CustomResource,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Debug,
    Clone,
    JsonSchema,
)]
#[admission_webhook_features(secret, service, admission_webhook_config)]
#[kube(group = "example.com", version = "v1", kind = "MyCr")]
pub struct CrSpec {
    pub name: String,
}

#[test]
fn it_has_a_function_for_creating_admission_webhook_tls_secret() {
    let secret: k8s_openapi::api::core::v1::Secret = MyCr::admission_webhook_secret("default");
    let data = secret.string_data.unwrap();
    assert_eq!(
        secret.metadata.name.unwrap(),
        "mycrs-example-com-admission-webhook-tls".to_string()
    );
    assert_eq!(secret.metadata.namespace.unwrap(), "default".to_string());
    assert!(&data.contains_key("tls.crt"), "secret contains certificate");
    assert!(&data.contains_key("tls.key"), "secret contains private key");
}

#[test]
fn it_has_a_function_for_creating_admission_webhook_service() {
    let service: k8s_openapi::api::core::v1::Service = MyCr::admission_webhook_service("default");

    assert_eq!(
        service.metadata.name.unwrap(),
        "mycrs-example-com-admission-webhook".to_string()
    );
    assert_eq!(service.metadata.namespace.unwrap(), "default".to_string());
    assert_eq!(
        service.spec.clone().unwrap().type_.unwrap(),
        "ClusterIP".to_string()
    );

    let spec = service.spec.unwrap();
    let selector = &spec.selector.unwrap();
    assert_eq!(selector.get("app").unwrap(), "mycrs-example-com-operator");
}

#[test]
fn it_has_a_function_for_creating_admission_webhook_configuration() {
    let service: k8s_openapi::api::core::v1::Service = MyCr::admission_webhook_service("default");
    let secret: k8s_openapi::api::core::v1::Secret = MyCr::admission_webhook_secret("default");
    let admission_webhook_configuration: MutatingWebhookConfiguration =
        MyCr::admission_webhook_configuration(service, secret).unwrap();

    let webhook = &admission_webhook_configuration.webhooks.unwrap()[0];
    let client_config = &webhook.client_config.clone();
    let service = client_config.service.clone().unwrap();

    let rule = &webhook.rules.clone().unwrap()[0];
    assert_eq!(
        admission_webhook_configuration.metadata.name.unwrap(),
        "mycrs.example.com".to_string()
    );
    assert_eq!(webhook.admission_review_versions, vec!["v1"]);
    assert_eq!(webhook.side_effects, "None");
    assert_eq!(
        rule.api_groups.clone().unwrap(),
        vec!["example.com".to_string()]
    );
    assert_eq!(rule.api_versions.clone().unwrap(), vec!["v1".to_string()]);
    assert_eq!(rule.operations.clone().unwrap(), vec!["*".to_string()]);
    assert_eq!(rule.resources.clone().unwrap(), vec!["mycrs".to_string()]);
    assert_eq!(rule.scope.clone().unwrap(), "Cluster".to_string());

    assert_eq!(client_config.url, None);
    assert_eq!(service.name, "mycrs-example-com-admission-webhook");
    assert_eq!(service.namespace, "default");
}

#[test]
fn it_has_a_function_for_creating_admission_webhook_resources() {
    let (service, secret, admission_webhook_configuration) =
        MyCr::admission_webhook_resources("default");

    assert_eq!(
        admission_webhook_configuration.metadata.name.unwrap(),
        "mycrs.example.com".to_string()
    );
    assert_eq!(
        service.metadata.name.unwrap(),
        "mycrs-example-com-admission-webhook".to_string()
    );
    assert_eq!(
        secret.metadata.name.unwrap(),
        "mycrs-example-com-admission-webhook-tls".to_string()
    );
}
