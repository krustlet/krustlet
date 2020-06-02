#![allow(missing_docs)]

use std::{
    convert::TryFrom,
    env,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    str,
};

use std::io::Write;

use dirs::home_dir;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::certificates::v1beta1::CertificateSigningRequest;
use kube::api::{Api, ListParams, PostParams, WatchEvent};
use kube::config::Kubeconfig;
use kube::runtime::Informer;
use kube::Config;
use log::{debug, info};
use rcgen::{
    Certificate, CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256,
};

use crate::config::Config as KubeletConfig;

const KUBECONFIG: &str = "KUBECONFIG";
const APPROVED_TYPE: &str = "Approved";

pub async fn bootstrap<K: AsRef<Path>>(
    config: &KubeletConfig,
    bootstrap_file: K,
) -> anyhow::Result<Config> {
    debug!("Starting bootstrap for {}", config.node_name);
    let exists = kubeconfig_exists();
    // kubelet searches for and finds a bootstrap-kubeconfig file
    if exists {
        debug!("Found existing kubeconfig, loading...");
        Config::infer()
            .await
            .map_err(|e| anyhow::anyhow!("Unable to load config from host: {}", e))
    } else {
        // TODO: if configured, kubelet automatically requests renewal of the certificate when it is close to expiry
        let original_kubeconfig = env::var(KUBECONFIG)?;
        debug!(
            "No existing kubeconfig found, loading bootstrap config from {:?}",
            bootstrap_file.as_ref()
        );
        env::set_var(KUBECONFIG, bootstrap_file.as_ref().as_os_str());
        let conf = kube::Config::infer().await?;
        let client = kube::Client::try_from(conf)?;

        let cert_bundle = gen_cert_bundle(config)?;
        let bootstrap_config = read_from(&bootstrap_file)?;
        let named_cluster = bootstrap_config
            .clusters
            .into_iter()
            .nth(0)
            .ok_or_else(|| {
                anyhow::anyhow!("Unable to find cluster information in bootstrap config")
            })?;
        let server = named_cluster.cluster.server;
        let ca_data = named_cluster
            .cluster
            .certificate_authority_data
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Unable to find certificate authority information in bootstrap config"
                )
            })?;

        let csrs: Api<CertificateSigningRequest> = Api::all(client);
        let csr_json = serde_json::json!({
          "apiVersion": "certificates.k8s.io/v1beta1",
          "kind": "CertificateSigningRequest",
          "metadata": {
            "name": config.node_name,
          },
          "spec": {
            "request": base64::encode(cert_bundle.serialize_request_pem()?.as_bytes()),
            "signerName": "kubernetes.io/kube-apiserver-client-kubelet",
            "usages": [
              "digital signature",
              "key encipherment",
              "client auth"
            ]
          }
        });

        let post_data = serde_json::from_value(csr_json)
            .expect("Invalid CSR JSON, this is a programming error");

        csrs.create(&PostParams::default(), &post_data).await?;

        // Wait for CSR signing
        let inf: Informer<CertificateSigningRequest> = Informer::new(csrs)
            .params(ListParams::default().fields(&format!("metadata.name={}", config.node_name)));

        let mut watcher = inf.poll().await?.boxed();
        let mut generated_kubeconfig = Vec::new();
        while let Some(event) = watcher.try_next().await? {
            match event {
                WatchEvent::Modified(m) | WatchEvent::Added(m) => {
                    // Do we have a cert?
                    let status = m.status.unwrap();
                    match status.certificate {
                        Some(cert) => {
                            match status.conditions {
                                Some(v) => {
                                    if v.into_iter()
                                        .find(|c| c.type_.as_str() == APPROVED_TYPE)
                                        .is_some()
                                    {
                                        generated_kubeconfig = gen_kubeconfig(
                                            ca_data,
                                            server,
                                            cert,
                                            cert_bundle.serialize_private_key_pem(),
                                        )?;
                                        break;
                                    } else {
                                        info!(
                                            "Got modified event, but CSR is not currently approved"
                                        );
                                    }
                                }
                                None => (),
                            };
                        }
                        None => (),
                    }
                }
                WatchEvent::Error(e) => {
                    return Err(anyhow::anyhow!(
                        "Error in event stream while waiting for certificate approval {}",
                        e
                    ));
                }
                WatchEvent::Deleted(_) => {}
                WatchEvent::Bookmark(_) => {}
            }
        }

        let write_path = original_kubeconfig.clone();
        let mut file = OpenOptions::new()
            .write(true) // <--------- this
            .create(true)
            .open(write_path)?;

        file.write_all(&generated_kubeconfig)?;
        // Set environment variable back to original value
        // so that infer will now pick up the file we generated
        env::set_var(KUBECONFIG, original_kubeconfig);

        Config::infer()
            .await
            .map_err(|e| anyhow::anyhow!("Unable to load generated config: {}", e))
    }
}

fn gen_cert_bundle(config: &KubeletConfig) -> anyhow::Result<Certificate> {
    let mut params = CertificateParams::default();
    params.not_before = chrono::Utc::now();
    params.not_after = chrono::Utc::now() + chrono::Duration::weeks(52);
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::OrganizationName, "system:nodes");
    distinguished_name.push(
        DnType::CommonName,
        &format!("system:node:{}", config.node_name),
    );
    params.distinguished_name = distinguished_name;
    params
        .key_pair
        .replace(KeyPair::generate(&PKCS_ECDSA_P256_SHA256)?);

    params.alg = &PKCS_ECDSA_P256_SHA256;

    Ok(Certificate::from_params(params)?)
}

fn gen_kubeconfig(
    ca_data: String,
    server: String,
    client_cert_data: k8s_openapi::ByteString,
    client_key: String,
) -> anyhow::Result<Vec<u8>> {
    let json = serde_json::json!({
        "kind": "Config",
        "apiVersion": "v1",
        "preferences": {},
        "clusters": [{
            "name": "krustlet",
            "cluster": {
                "certificate-authority-data": ca_data,
                "server": server,
            }
        }],
        "users":[{
            "name": "krustlet",
            "user": {
                "client-certificate-data": client_cert_data,
                "client-key-data": base64::encode(client_key.as_bytes())
            }
        }],
        "contexts": [{
            "name": "krustlet",
            "context": {
                "cluster": "krustlet",
                "user": "krustlet",
            }
        }],
        "current-context": "krustlet"
    });

    serde_json::to_vec(&json)
        .map_err(|e| anyhow::anyhow!("Unable to serialize generated kubeconfig: {}", e))
}

/// Search the kubeconfig file
fn kubeconfig_exists() -> bool {
    kubeconfig_path()
        .or_else(default_kube_path)
        .unwrap_or_default()
        .exists()
}

/// Returns kubeconfig path from specified environment variable.
fn kubeconfig_path() -> Option<PathBuf> {
    env::var_os(KUBECONFIG).map(PathBuf::from)
}

/// Returns kubeconfig path from `$HOME/.kube/config`.
fn default_kube_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".kube").join("config"))
}

fn read_from<P: AsRef<Path>>(path: P) -> anyhow::Result<Kubeconfig> {
    let f = File::open(path)
        .map_err(|e| anyhow::anyhow!(format!("Error loading bootstrap-kubelet.conf: {}", e)))?;
    let config = serde_yaml::from_reader(f)
        .map_err(|e| anyhow::anyhow!(format!("Error parsing bootstrap-kubelet.conf: {}", e)))?;
    Ok(config)
}
