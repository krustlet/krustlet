#![allow(missing_docs)]

use std::{
    convert::TryFrom,
    env,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    str,
};

use std::io::{prelude::*, Seek, SeekFrom};

use dirs::home_dir;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::certificates::v1beta1::CertificateSigningRequest;
use kube::api::{Api, ListParams, PostParams, WatchEvent};
use kube::config::Kubeconfig;
use kube::runtime::Informer;
use kube::Config;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::{
    rsa::Rsa,
    x509::{X509NameBuilder, X509ReqBuilder},
};

const BOOTSTRAP_TOKEN_FILE: &str = "/etc/kubernetes/bootstrap-kubelet.conf";
const KUBECONFIG: &str = "KUBECONFIG";

pub async fn bootstrap(hostname: &str) -> anyhow::Result<Config> {
    // kubelet searches for and finds a bootstrap-kubeconfig file
    if kubeconfig_exists() && false {
        Config::infer()
            .await
            .map_err(|e| anyhow::anyhow!("Unable to load config from host: {}", e))
    } else {
        // else
        // kubelet reads its bootstrap file, retrieving the URL of the API server and a limited usage “token”
        // kubelet now has limited credentials to create and retrieve a certificate signing request (CSR)
        // kubelet creates a CSR for itself with the signerName set to kubernetes.io/kube-apiserver-client-kubelet
        // CSR is approved in one of two ways:
        // If configured, kube-controller-manager automatically approves the CSR
        // If configured, an outside process, possibly a person, approves the CSR using the Kubernetes API or via kubectl
        // Certificate is created for the kubelet
        // Certificate is issued to the kubelet
        // kubelet retrieves the certificate
        // kubelet creates a proper kubeconfig with the key and signed certificate
        // kubelet begins normal operation
        // TODO: if configured, kubelet automatically requests renewal of the certificate when it is close to expiry
        // The renewed certificate is approved and issued, either automatically or manually, depending on configuration.
        let original_kubeconfig = env::var(KUBECONFIG)?;
        env::set_var(KUBECONFIG, BOOTSTRAP_TOKEN_FILE);
        let conf = kube::Config::infer().await?;
        let client = kube::Client::try_from(conf)?;
        // Function handles defaulting
        let privkey = gen_pkey()?;
        let csr = gen_csr(hostname, &privkey).unwrap();
        let bootstrap_token_path = Path::new(BOOTSTRAP_TOKEN_FILE);
        let bootstrap_config = read_from(&bootstrap_token_path)?;
        let named_cluster = bootstrap_config.clusters.iter().nth(0).unwrap();
        let server = &named_cluster.cluster.server;
        let err = &"error".to_owned();
        let ca_data = named_cluster
            .cluster
            .certificate_authority_data
            .as_ref()
            .unwrap_or_else(|| err);

        let csrs: Api<CertificateSigningRequest> = Api::all(client);
        let csr_json = serde_json::json!({
          "apiVersion": "certificates.k8s.io/v1beta1",
          "kind": "CertificateSigningRequest",
          "metadata": {
            "name": hostname,
          },
          "spec": {
            "request": csr,
            "signerName": "kubernetes.io/kube-apiserver-client-kubelet",
            "usages": [
              "digital signature",
              "key encipherment",
              "server auth"
            ]
          }
        });

        let post_data =
            serde_json::from_value(csr_json).expect("Unable to generate valid CSR JSON");

        csrs.create(&PostParams::default(), &post_data).await?;
        //println!("{:?}", resp.status.unwrap().certificate.unwrap());

        // Wait for CSR signing
        let inf: Informer<CertificateSigningRequest> = Informer::new(csrs)
            .params(ListParams::default().fields(&format!("metadata.name={}", hostname)));

        let mut watcher = inf.poll().await?.boxed();
        let mut approved = false;
        let mut generated_kubeconfig = serde_json::json!({});
        while let Some(event) = watcher.try_next().await? {
            match event {
                WatchEvent::Modified(m) => {
                    // Do we have a cert?
                    let status = m.status.unwrap();
                    match status.certificate {
                        Some(certificate) => {
                            match status.conditions {
                                Some(v) => {
                                    let condition = v.iter().nth(0).unwrap();
                                    let reason = &condition.reason.as_ref();
                                    match reason.unwrap().as_str() {
                                        "KubectlApprove" => {
                                            generated_kubeconfig = gen_kubeconfig(
                                                ca_data,
                                                server,
                                                certificate,
                                                privkey,
                                            )?;
                                            println!("{:?}", generated_kubeconfig);
                                            approved = true;
                                            break;
                                        }
                                        _ => {
                                            println!("Condition other than Approved: {:?}", reason);
                                        }
                                    }
                                }
                                None => (),
                            };
                        }
                        None => (),
                    }
                    //cert = m;
                }
                WatchEvent::Error(e) => {
                    println!("{}", 88);
                    println!("{:?}", e);
                    // Exit with error message
                }
                WatchEvent::Added(_) => {}
                WatchEvent::Deleted(_) => {}
                WatchEvent::Bookmark(_) => {}
            }
        }

        if approved {
            let write_path = original_kubeconfig.clone();
            // TODO: Generate Kubeconfig
            let mut file = OpenOptions::new()
                .read(true)
                .write(true) // <--------- this
                .create(true)
                .open(write_path)?;

            file.seek(SeekFrom::Start(0)).unwrap();
            let file_contents = generated_kubeconfig.to_string();
            let bytes = file_contents.as_bytes();
            //TODO: Currently you get an os error 13 if you run this a second time.
            //This shouldn't even execute if the file exists.
            file.write_all(bytes).unwrap();
            // Set environement variable back to original value
            // so that infer will now pick up the file we generated
            env::set_var(KUBECONFIG, original_kubeconfig);
        }

        Config::infer()
            .await
            .map_err(|e| anyhow::anyhow!("Unable to load generated config: {}", e))
    }
}

pub fn gen_pkey() -> anyhow::Result<PKey<Private>> {
    let rsa = Rsa::generate(2048)?;
    Ok(PKey::from_rsa(rsa)?)
}

pub fn gen_csr(hostname: &str, privkey: &PKey<Private>) -> anyhow::Result<String> {
    let mut req_builder = X509ReqBuilder::new()?;
    req_builder.set_pubkey(privkey)?;

    let mut x509_name = X509NameBuilder::new()?;
    x509_name.append_entry_by_text("CN", hostname)?;
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

pub fn gen_kubeconfig(
    ca_data: &String,
    server: &String,
    client_cert_data: k8s_openapi::ByteString,
    client_key: PKey<Private>,
) -> anyhow::Result<serde_json::Value> {
    let pem = &client_key.private_key_to_pem_pkcs8()?;
    let pkey = str::from_utf8(pem)?;
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
                "client-key-data": base64::encode(pkey)
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

    Ok(json)
}

/// Search the kubeconfig file
pub fn kubeconfig_exists() -> bool {
    kubeconfig_path()
        .or_else(default_kube_path)
        .unwrap_or_else(|| PathBuf::from("/foo/bar"))
        .exists()
}

/// Returns kubeconfig path from specified environment variable.
pub fn kubeconfig_path() -> Option<PathBuf> {
    env::var_os(KUBECONFIG).map(PathBuf::from)
}

/// Returns kubeconfig path from `$HOME/.kube/config`.
pub fn default_kube_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".kube").join("config"))
}

pub fn read_from<P: AsRef<Path>>(path: P) -> anyhow::Result<Kubeconfig> {
    let f = File::open(path)
        .map_err(|e| anyhow::anyhow!(format!("Error loading bootstrap-kubelet.conf: {}", e)))?;
    let config = serde_yaml::from_reader(f)
        .map_err(|e| anyhow::anyhow!(format!("Error parsing bootstrap-kubelet.conf: {}", e)))?;
    Ok(config)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_gen_csr() {
        let privkey = gen_pkey().unwrap();
        let csr = gen_csr("test.com", &privkey).unwrap();

        println!("csr: {}", csr);
        assert_ne!(0, csr.len());
    }

    #[tokio::test]
    async fn test_bootstrap() {
        let config = bootstrap("test.com").await;

        println!("config: {:?}", config.unwrap());
        assert_ne!(true, false);
        //assert_ne!(0, csr.len());
    }
}
