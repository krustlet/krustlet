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
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::{
    rsa::Rsa,
    x509::{X509NameBuilder, X509ReqBuilder},
};

const KUBECONFIG: &str = "KUBECONFIG";
const APPROVED_TYPE: &str = "Approved";

pub async fn bootstrap<K: AsRef<Path>>(
    node_name: &str,
    bootstrap_file: K,
) -> anyhow::Result<Config> {
    debug!("Starting bootstrap for {}", node_name);
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
        // Function handles defaulting
        let privkey = gen_pkey()?;
        let csr = gen_csr(node_name, &privkey)?;
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
            "name": node_name,
          },
          "spec": {
            "request": csr,
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
            .params(ListParams::default().fields(&format!("metadata.name={}", node_name)));

        let mut watcher = inf.poll().await?.boxed();
        let mut generated_kubeconfig = Vec::new();
        while let Some(event) = watcher.try_next().await? {
            match event {
                WatchEvent::Modified(m) | WatchEvent::Added(m) => {
                    // Do we have a cert?
                    let status = m.status.unwrap();
                    match status.certificate {
                        Some(certificate) => {
                            match status.conditions {
                                Some(v) => {
                                    if v.into_iter()
                                        .find(|c| c.type_.as_str() == APPROVED_TYPE)
                                        .is_some()
                                    {
                                        generated_kubeconfig =
                                            gen_kubeconfig(ca_data, server, certificate, privkey)?;
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

fn gen_pkey() -> anyhow::Result<PKey<Private>> {
    let rsa = Rsa::generate(2048)?;
    Ok(PKey::from_rsa(rsa)?)
}

fn gen_csr(node_name: &str, privkey: &PKey<Private>) -> anyhow::Result<String> {
    let mut req_builder = X509ReqBuilder::new()?;
    req_builder.set_pubkey(privkey)?;

    let mut x509_name = X509NameBuilder::new()?;
    x509_name.append_entry_by_text("CN", &format!("system:node:{}", node_name))?;
    x509_name.append_entry_by_text("O", "system:nodes")?;
    let x509_name = x509_name.build();
    req_builder.set_subject_name(&x509_name)?;

    req_builder.sign(&privkey, MessageDigest::sha256())?;
    let req = req_builder.build();

    let pem = req.to_pem().unwrap();
    let csr = match String::from_utf8(pem) {
        Ok(v) => base64::encode(v),
        Err(e) => e.to_string(),
    };

    Ok(csr)
}

fn gen_kubeconfig(
    ca_data: String,
    server: String,
    client_cert_data: k8s_openapi::ByteString,
    client_key: PKey<Private>,
) -> anyhow::Result<Vec<u8>> {
    let pem = client_key.private_key_to_pem_pkcs8()?;
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
                "client-key-data": base64::encode(&pem)
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
