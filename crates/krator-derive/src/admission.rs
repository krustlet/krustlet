use crate::proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    token::Comma,
    Attribute, Data, DeriveInput, Path, Result,
};

const ATTRIBUTE_NAME: &str = "admission_webhook_features";

pub trait CustomDerive: Sized {
    fn parse(input: syn::DeriveInput) -> Result<Self>;
    fn emit(self) -> Result<proc_macro2::TokenStream>;
}

#[derive(Debug)]
pub struct CustomResourceInfos {
    pub name: String,
    pub features: Features,
}

pub(crate) fn run_custom_derive<T>(input: TokenStream) -> TokenStream
where
    T: CustomDerive,
{
    let input: proc_macro2::TokenStream = input.into();
    let token_stream = match syn::parse2(input)
        .and_then(|input| <T as CustomDerive>::parse(input))
        .and_then(<T as CustomDerive>::emit)
    {
        Ok(token_stream) => token_stream,
        Err(err) => err.to_compile_error(),
    };

    token_stream.into()
}

trait ResultExt<T> {
    fn spanning(self, spanned: impl quote::ToTokens) -> Result<T>;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: std::fmt::Display,
{
    fn spanning(self, spanned: impl quote::ToTokens) -> Result<T> {
        self.map_err(|err| syn::Error::new_spanned(spanned, err))
    }
}
#[derive(Default, Debug)]
pub struct Features {
    pub secret: bool,
    pub service: bool,
    pub admission_webhook_config: bool,
    pub attributes_found: bool,
}

impl Parse for Features {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut features = Features {
            ..Default::default()
        };
        input
            .parse_terminated::<Path, Comma>(|i| i.parse::<Path>())?
            .into_iter()
            .for_each(|ref path| {
                if let Some(ident) = path.get_ident() {
                    features.attributes_found = true;
                    match &*ident.to_string() {
                        "secret" => features.secret = true,
                        "service" => features.service = true,
                        "admission_webhook_config" => features.admission_webhook_config = true,
                        _ => {
                            panic!(format!("WebhookAdmission attribute {} can only contain one or more of the following values: service, secret, admission_webhook_config", ATTRIBUTE_NAME))
                        }
                    };
                }
            });

        Ok(features)
    }
}

fn get_features(attrs: &Vec<Attribute>) -> Features {
    attrs
        .into_iter()
        .fold(Features::default(), |mut acc, ref attr| {
            match parse_as_features_attr(attr) {
                Some(features) => {
                    acc.secret |= features.secret;
                    acc.service |= features.service;
                    acc.admission_webhook_config |= features.admission_webhook_config;
                    acc.attributes_found |= features.attributes_found;
                }
                None => {}
            };
            acc
        })
}

fn parse_as_features_attr(attr: &Attribute) -> Option<Features> {
    if let Some(id) = attr.path.get_ident() {
        if id == ATTRIBUTE_NAME {
            attr.parse_args::<Features>().ok()
        } else {
            None
        }
    } else {
        None
    }
}

impl CustomDerive for CustomResourceInfos {
    fn parse(input: DeriveInput) -> Result<Self> {
        let ident = input.ident;

        // Limit derive to structs
        let _s = match input.data {
            Data::Struct(ref s) => s,
            _ => {
                return Err(r#"Enums or Unions can not #[derive(AdmissionWebhook)"#).spanning(ident)
            }
        };

        let features = get_features(&input.attrs);
        if !features.attributes_found {
            panic!("missing AdmissionWebhook attribute 'admission_webhook_features'")
        }

        // Outputs
        let mut cri = CustomResourceInfos {
            name: "".to_string(),
            features: features,
        };

        let mut name: Option<String> = None;

        // Arg parsing
        for attr in &input.attrs {
            if let syn::AttrStyle::Outer = attr.style {
            } else {
                continue;
            }
            if !attr.path.is_ident("kube") {
                continue;
            }
            let metas = match attr.parse_meta()? {
                syn::Meta::List(meta) => meta.nested,
                meta => {
                    return Err(r#"#[kube] expects a list of metas, like `#[kube(...)]`"#)
                        .spanning(meta)
                }
            };

            for meta in metas {
                match &meta {
                    // key-value arguments
                    syn::NestedMeta::Meta(syn::Meta::NameValue(meta)) => {
                        if meta.path.is_ident("kind") {
                            if let syn::Lit::Str(lit) = &meta.lit {
                                name = Some(lit.value());
                                break;
                            } else {
                                return Err(
                                    r#"#[kube(kind = "...")] expects a string literal value"#,
                                )
                                .spanning(meta);
                            }
                        }
                    } // unknown arg
                    _ => (),
                };
            }
        }
        cri.name = name.expect("kube macro must have property name set");

        Ok(cri)
    }

    fn emit(self) -> Result<proc_macro2::TokenStream> {
        let name = self.name;
        let name_identifier = format_ident!("{}", name);

        let mut token_stream = quote! {};

        if self.features.secret {
            token_stream = quote! {
                #token_stream

                /// Returns the name of the admission webhook secret that will get created with `admission_webhook_secret()`
                pub fn admission_webhook_secret_name() -> std::string::String {
                    let crd = #name_identifier::crd();
                    format!("{}-{}-admission-webhook-tls", crd.spec.names.plural, crd.spec.group).to_string().replace(".", "-")
                }

                /// Returns a Kubernetes secret of type `tls` that contains a certificate and a private key and
                /// can be used for the admission webhook service
                pub fn admission_webhook_secret(namespace: &str) -> k8s_openapi::api::core::v1::Secret {
                    let crd = #name_identifier::crd();
                    let service_name = #name_identifier::admission_webhook_service_name();

                    let subject_alt_names = vec![
                        service_name.clone(),
                        format!("{}.{}", &service_name, namespace).to_string(),
                        format!("{}.{}.svc", &service_name, namespace).to_string(),
                        format!("{}.{}.svc.cluster", &service_name, namespace).to_string(),
                        format!("{}.{}.svc.cluster.local", &service_name, namespace).to_string(),
                    ];
                    let cert = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();

                    let mut data = std::collections::BTreeMap::new();
                    data.insert("tls.crt".into(), cert.serialize_pem().unwrap());
                    data.insert("tls.key".into(), cert.serialize_private_key_pem());

                    k8s_openapi::api::core::v1::Secret {
                        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                            name: Some(#name_identifier::admission_webhook_secret_name()),
                            namespace: Some(namespace.to_string()),
                            ..Default::default()
                        },
                        string_data: Some(data),
                        type_: Some("tls".to_string()),
                        ..Default::default()
                    }
                }
            };
        }

        if self.features.service {
            token_stream = quote! {
                #token_stream

                /// Returns the selector value for the label `app` that the service created with `admission_webhook_service()`
                /// uses for selecting the pods that serve the admission webhook
                pub fn admission_webhook_service_app_selector() -> std::string::String {
                    let crd = #name_identifier::crd();
                    format!("{}-{}-operator", crd.spec.names.plural, crd.spec.group).to_string().replace(".", "-")
                }

                /// Returns the name of the admission webhook service that will get created with `admission_webhook_service()`
                pub fn admission_webhook_service_name() -> std::string::String {
                    let crd = #name_identifier::crd();
                    format!("{}-{}-admission-webhook", crd.spec.names.plural, crd.spec.group).to_string().replace(".", "-")
                }

                /// Returns a service that forwards to pods where label `app` has the value returned by the function
                /// `admission_webhook_service_app_selector()`. It exposes port `443` (necessary for webhooks) and
                /// listens to the pod's port `8443`
                pub fn admission_webhook_service(namespace: &str) -> k8s_openapi::api::core::v1::Service {
                    let crd = #name_identifier::crd();

                    let mut selector = std::collections::BTreeMap::new();
                    selector.insert("app".into(), #name_identifier::admission_webhook_service_app_selector());

                    k8s_openapi::api::core::v1::Service {
                        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                            name: Some(#name_identifier::admission_webhook_service_name()),
                            namespace: Some(namespace.to_string()),
                            ..Default::default()
                        },
                        spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                            selector: Some(selector),
                            ports: Some(vec![k8s_openapi::api::core::v1::ServicePort{
                                protocol: Some("TCP".to_string()),
                                port: 443,
                                target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(8443)),
                                ..Default::default()
                            }]),
                            type_: Some("ClusterIP".to_string()),
                            ..Default::default()
                        }),
                        status: None
                   }
                }
            };
        }

        if self.features.admission_webhook_config {
            token_stream = quote! {
                #token_stream

                /// Returns the name of the admission webhook configuration that will get created with `admission_webhook_configuration()`
                pub fn admission_webhook_configuration_name() -> std::string::String {
                    let crd = #name_identifier::crd();
                    format!("{}.{}", crd.spec.names.plural, crd.spec.group).to_string()
                }


                /// Creates a MutatingWebhookConfiguration using the certificate from the given service and the service
                /// of the given service as configuration
                pub fn admission_webhook_configuration(service: k8s_openapi::api::core::v1::Service, secret: k8s_openapi::api::core::v1::Secret) -> Result<k8s_openapi::api::admissionregistration::v1::MutatingWebhookConfiguration, Box<dyn std::error::Error + Send + Sync>> {
                   let crd = #name_identifier::crd();

                   let webhook_name = #name_identifier::admission_webhook_configuration_name() ;
                   let versions: std::vec::Vec<std::string::String> = crd.spec.versions.into_iter().map(|v| v.name).collect();

                   if service.metadata.name.is_none() { return Err("service does not have a name set".into()) };
                   if service.metadata.namespace.is_none() { return Err("service does not have a namespace set".into()) };

                   if secret.metadata.name.is_none() { return Err("secret does not have a name set".into()) };
                   if secret.metadata.namespace.is_none() { return Err("secret does not have a namespace set".into()) };

                   if secret.type_ != Some("tls".to_string()) { return Err(format!("secret {}/{} is not a tls secret", secret.metadata.namespace.as_ref().unwrap(), secret.metadata.name.as_ref().unwrap()).into()) };

                   const TLS_CRT: &'static str = "tls.crt";

                   let ca_bundle = secret
                       .string_data
                       .as_ref()
                       .and_then(|ref string_data| {
                           string_data
                               .get(TLS_CRT)
                               .map(std::string::String::as_bytes)
                               .map(std::vec::Vec::from)
                               .map(k8s_openapi::ByteString)
                       })
                       .or(secret
                           .data
                           .as_ref()
                           .and_then(|ref data| data.get(TLS_CRT).map(k8s_openapi::ByteString::to_owned)));


                   if ca_bundle.is_none() { return Err(format!("secret with {} is does not contain data 'tls.crt'", secret.metadata.name.unwrap()).into())}

                   Ok(k8s_openapi::api::admissionregistration::v1::MutatingWebhookConfiguration{
                       metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                           name: Some(webhook_name.clone()),
                           ..Default::default()
                       },
                       webhooks: Some(vec![
                        k8s_openapi::api::admissionregistration::v1::MutatingWebhook{
                           admission_review_versions: versions.clone(),
                           name: format!("{}", webhook_name.clone()),
                           side_effects: "None".to_string(),
                           rules: Some(vec![k8s_openapi::api::admissionregistration::v1::RuleWithOperations{
                               api_groups: Some(vec![crd.spec.group]),
                               api_versions: Some(versions),
                               operations: Some(vec!["*".to_string()]),
                               resources: Some(vec![crd.spec.names.plural]),
                               scope: Some(crd.spec.scope)
                            }]),
                           client_config: k8s_openapi::api::admissionregistration::v1::WebhookClientConfig{
                               ca_bundle: ca_bundle,
                               service: Some(k8s_openapi::api::admissionregistration::v1::ServiceReference{
                                   name: service.metadata.name.unwrap(),
                                   namespace: service.metadata.namespace.unwrap(),
                                   path: Some("/".to_string()),
                                   ..Default::default()
                               }),
                               url: None
                           },
                           ..Default::default()
                        }
                      ])
                   })
                }
            };
        }

        if self.features.secret && self.features.service && self.features.admission_webhook_config {
            token_stream = quote! {
                #token_stream

                /// Convenience function that returns all Kubernetes resources necessary to configure an admission webhook
                /// service. It expects a deployed pod with label `app` having the value returned by `admission_webhook_service_name()`.
                pub fn admission_webhook_resources(namespace: &str) -> (k8s_openapi::api::core::v1::Service, k8s_openapi::api::core::v1::Secret, k8s_openapi::api::admissionregistration::v1::MutatingWebhookConfiguration){
                    let service: k8s_openapi::api::core::v1::Service = #name_identifier::admission_webhook_service(namespace);
                    let secret: k8s_openapi::api::core::v1::Secret = #name_identifier::admission_webhook_secret(namespace);
                    let admission_webhook_configuration = #name_identifier::admission_webhook_configuration(service.clone(), secret.clone()).unwrap();

                    (service, secret, admission_webhook_configuration)
                }
            };
        }

        token_stream = quote! {

            impl #name_identifier {
                #token_stream
            }
        };

        Ok(token_stream.into())
    }
}
