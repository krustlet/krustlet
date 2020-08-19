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
        ClientConfig { protocol }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn empty_config() -> Config {
        // We can't use Config::default() because it can panic when trying
        // to derive a node IP address
        Config {
            allow_local_modules: false,
            bootstrap_file: std::path::PathBuf::from("/nope"),
            data_dir: std::path::PathBuf::from("/nope"),
            hostname: "nope".to_owned(),
            insecure_registries: None,
            max_pods: 0,
            node_ip: "127.0.0.1".parse().unwrap(),
            node_labels: std::collections::HashMap::new(),
            node_name: "nope".to_owned(),
            server_config: crate::config::ServerConfig {
                addr: "127.0.0.1".parse().unwrap(),
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
        let mut config = empty_config();
        config.insecure_registries = Some(vec!["local".to_owned(), "dev".to_owned()]);
        let client_config = config.client_config();

        let expected_protocol =
            ClientProtocol::HttpsExcept(vec!["local".to_owned(), "dev".to_owned()]);
        assert_eq!(expected_protocol, client_config.protocol);
    }
}
