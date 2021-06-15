//! Functions for constructing 'useful' objects from configuration
//! data in 'sensible' ways.

use crate::config::Config;
use oci_distribution::client::{ClientConfig, ClientConfigSource, ClientProtocol};

impl ClientConfigSource for Config {
    fn client_config(&self) -> ClientConfig {
        let protocol = match &self.insecure_registries {
            None => ClientProtocol::default(),
            Some(registries) => ClientProtocol::HttpsExcept(registries.clone()),
        };
        ClientConfig {
            protocol,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr};

    fn empty_config() -> Config {
        // We can't use Config::default() because it can panic when trying
        // to derive a node IP address
        Config {
            allow_local_modules: false,
            bootstrap_file: std::path::PathBuf::from("/nope"),
            data_dir: std::path::PathBuf::from("/nope"),
            hostname: "nope".to_owned(),
            insecure_registries: None,
            plugins_dir: std::path::PathBuf::from("/nope"),
            device_plugins_dir: std::path::PathBuf::from("/nope"),
            max_pods: 0,
            node_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            node_labels: std::collections::HashMap::new(),
            node_name: "nope".to_owned(),
            server_config: crate::config::ServerConfig {
                addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 0,
                cert_file: std::path::PathBuf::from("/nope"),
                private_key_file: std::path::PathBuf::from("/nope"),
            },
        }
    }

    #[test]
    fn oci_config_defaults_to_https() {
        let config = empty_config();
        let client_config = config.client_config();
        assert_eq!(ClientProtocol::Https, client_config.protocol);
    }

    #[test]
    fn oci_config_respects_config_insecure_registries() {
        let config = Config {
            insecure_registries: Some(vec!["local".to_owned(), "dev".to_owned()]),
            ..empty_config()
        };

        let client_config = config.client_config();

        let expected_protocol =
            ClientProtocol::HttpsExcept(vec!["local".to_owned(), "dev".to_owned()]);
        assert_eq!(expected_protocol, client_config.protocol);
    }
}
