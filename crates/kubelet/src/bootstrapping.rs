#![allow(missing_docs)]

use std::{
    convert::TryFrom,
    env,
    path::{Path, PathBuf},
    str,
};

use dirs::home_dir;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::certificates::v1beta1::CertificateSigningRequest;
use kube::api::{Api, ListParams, PostParams, WatchEvent};
use kube::config::Kubeconfig;
use kube::runtime::Informer;
use kube::Config;
use log::{debug, info};
use rcgen::{
    Certificate, CertificateParams, DistinguishedName, DnType, KeyPair, SanType,
    PKCS_ECDSA_P256_SHA256,
};
use tokio::fs::{read, write};

use crate::config::Config as KubeletConfig;

const KUBECONFIG: &str = "KUBECONFIG";
const APPROVED_TYPE: &str = "Approved";

pub async fn bootstrap<K: AsRef<Path>>(
    config: &KubeletConfig,
    bootstrap_file: K,
) -> anyhow::Result<Config> {
    debug!("Starting bootstrap for {}", config.node_name);
    let kubeconfig = bootstrap_auth(config, bootstrap_file).await?;
    bootstrap_tls(config, kubeconfig.clone()).await?;
    Ok(kubeconfig)
}

async fn bootstrap_auth<K: AsRef<Path>>(
    config: &KubeletConfig,
    bootstrap_file: K,
) -> anyhow::Result<Config> {
    if kubeconfig_exists() {
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

        let cert_bundle = gen_auth_cert(config)?;
        let bootstrap_config = read_from(&bootstrap_file).await?;
        let named_cluster = bootstrap_config
            .clusters
            .into_iter()
            .next()
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
                    if let Some(cert) = status.certificate {
                        if let Some(v) = status.conditions {
                            if v.into_iter().any(|c| c.type_.as_str() == APPROVED_TYPE) {
                                generated_kubeconfig = gen_kubeconfig(
                                    ca_data,
                                    server,
                                    cert,
                                    cert_bundle.serialize_private_key_pem(),
                                )?;
                                break;
                            } else {
                                info!("Got modified event, but CSR is not currently approved");
                            }
                        }
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

        write(&original_kubeconfig, &generated_kubeconfig).await?;
        // Set environment variable back to original value
        // so that infer will now pick up the file we generated
        env::set_var(KUBECONFIG, original_kubeconfig);

        Config::infer()
            .await
            .map_err(|e| anyhow::anyhow!("Unable to load generated config: {}", e))
    }
}

async fn bootstrap_tls(config: &KubeletConfig, kubeconfig: Config) -> anyhow::Result<()> {
    debug!("Starting bootstrap of TLS serving certs");
    if config.server_config.tls_cert_file.exists() {
        return Ok(());
    }

    let cert_bundle = gen_tls_cert(config)?;

    let csr_name = format!("{}-tls", config.hostname);
    let client = kube::Client::try_from(kubeconfig)?;
    let csrs: Api<CertificateSigningRequest> = Api::all(client);
    let csr_json = serde_json::json!({
        "apiVersion": "certificates.k8s.io/v1beta1",
        "kind": "CertificateSigningRequest",
        "metadata": {
            "name": csr_name,
        },
        "spec": {
        "request": base64::encode(cert_bundle.serialize_request_pem()?.as_bytes()),
        "signerName": "kubernetes.io/kubelet-serving",
        "usages": [
            "digital signature",
            "key encipherment",
            "server auth"
        ]
        }
    });

    let post_data =
        serde_json::from_value(csr_json).expect("Invalid CSR JSON, this is a programming error");

    csrs.create(&PostParams::default(), &post_data).await?;

    // Wait for CSR signing
    let inf: Informer<CertificateSigningRequest> = Informer::new(csrs)
        .params(ListParams::default().fields(&format!("metadata.name={}", csr_name)));

    let mut watcher = inf.poll().await?.boxed();
    let mut certificate = String::new();
    while let Some(event) = watcher.try_next().await? {
        match event {
            WatchEvent::Modified(m) | WatchEvent::Added(m) => {
                // Do we have a cert?
                let status = m.status.unwrap();
                if let Some(cert) = status.certificate {
                    if let Some(v) = status.conditions {
                        if v.into_iter().any(|c| c.type_.as_str() == APPROVED_TYPE) {
                            certificate = std::str::from_utf8(&cert.0)?.to_owned();
                            break;
                        } else {
                            info!("Got modified event, but CSR is not currently approved");
                        }
                    }
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
    let private_key = cert_bundle.serialize_private_key_pem();
    debug!(
        "Got certificate from API, writing cert to {:?} and private key to {:?}",
        config.server_config.tls_cert_file, config.server_config.tls_private_key_file
    );
    write(&config.server_config.tls_cert_file, &certificate).await?;
    write(&config.server_config.tls_private_key_file, &private_key).await?;

    Ok(())
}

fn gen_auth_cert(config: &KubeletConfig) -> anyhow::Result<Certificate> {
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

fn gen_tls_cert(config: &KubeletConfig) -> anyhow::Result<Certificate> {
    let mut params = CertificateParams::default();
    params.not_before = chrono::Utc::now();
    params.not_after = chrono::Utc::now() + chrono::Duration::weeks(52);
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::OrganizationName, "system:nodes");
    distinguished_name.push(
        DnType::CommonName,
        &format!("system:node:{}", config.hostname),
    );
    params.distinguished_name = distinguished_name;
    params
        .key_pair
        .replace(KeyPair::generate(&PKCS_ECDSA_P256_SHA256)?);

    params.alg = &PKCS_ECDSA_P256_SHA256;

    params.subject_alt_names = vec![
        SanType::DnsName(config.hostname.clone()),
        SanType::IpAddress(config.node_ip),
    ];

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

async fn read_from<P: AsRef<Path>>(path: P) -> anyhow::Result<Kubeconfig> {
    // Serde yaml doesn't have async support so we have to read the whole file in
    let raw = read(path)
        .await
        .map_err(|e| anyhow::anyhow!(format!("Error loading bootstrap file: {}", e)))?;
    let config = serde_yaml::from_slice(&raw)
        .map_err(|e| anyhow::anyhow!(format!("Error parsing bootstrap file: {}", e)))?;

    Ok(config)
}
