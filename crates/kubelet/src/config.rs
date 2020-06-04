//! Configuration for a Kubelet
//!
//! The best way to configure the kubelet is by using [`Config::default_config`]
//! or by turning on the "cli" feature and using [`Config::new_from_flags`].

use std::net::IpAddr;
use std::net::ToSocketAddrs;
use std::path::PathBuf;

#[cfg(feature = "cli")]
use structopt::StructOpt;

use std::collections::HashMap;

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_MAX_PODS: u16 = 110;
const BOOTSTRAP_FILE: &str = "/etc/kubernetes/bootstrap-kubelet.conf";

/// The configuration needed for a kubelet to run properly.
///
/// This can be configured manually in your code or if you are exposing a CLI, use the
/// [`Config::new_from_flags`] (this requires the "cli" feature to
/// be enabled).
///
/// Use [`Config::default_config`] to generate a config with all
/// of the default values set.
#[derive(Clone, Debug)]
pub struct Config {
    /// The ip address the node is exposed on
    pub node_ip: IpAddr,
    /// The hostname of the node
    pub hostname: String,
    /// The node's name
    pub node_name: String,
    /// The Kubelet server configuration
    pub server_config: ServerConfig,
    /// The directory where the Kubelet will store data
    pub data_dir: PathBuf,
    /// Labels to add when registering the node in the cluster
    pub node_labels: HashMap<String, String>,
    /// The maximum pods for this kubelet (reported to apiserver)
    pub max_pods: u16,
    /// The location of the tls bootstrapping file
    pub bootstrap_file: PathBuf,
}
/// The configuration for the Kubelet server.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The ip address the Kubelet server is running on
    pub addr: IpAddr,
    /// The port the Kubelet server is running on
    pub port: u16,
    /// Path to kubelet TLS certificate.
    pub tls_cert_file: PathBuf,
    /// Path to kubelet TLS private key.
    pub tls_private_key_file: PathBuf,
}

impl Config {
    /// Returns a Config object set with all of the defaults.
    ///
    /// Useful for cases when you don't want to set most of the values yourself. The
    /// preferred_ip_family argument takes an IpAddr that is either V4 or V6 to
    /// indicate the preferred IP family to use for defaults
    pub fn default_config(preferred_ip_family: &IpAddr) -> anyhow::Result<Self> {
        let hostname = default_hostname()?;
        let data_dir = default_data_dir()?;
        let tls_cert_file = default_cert_path(&data_dir);
        let tls_private_key_file = default_key_path(&data_dir);
        Ok(Config {
            node_ip: default_node_ip(&mut hostname.clone(), preferred_ip_family)?,
            node_name: sanitize_hostname(&hostname),
            node_labels: HashMap::new(),
            hostname,
            data_dir,
            max_pods: DEFAULT_MAX_PODS,
            bootstrap_file: PathBuf::from(BOOTSTRAP_FILE),
            server_config: ServerConfig {
                addr: match preferred_ip_family {
                    // Just unwrap these because they are programmer error if they
                    // don't parse
                    IpAddr::V4(_) => "0.0.0.0".parse().unwrap(),
                    IpAddr::V6(_) => "::".parse().unwrap(),
                },
                port: DEFAULT_PORT,
                tls_cert_file,
                tls_private_key_file,
            },
        })
    }

    /// Parses all command line flags and sets the proper defaults. The version
    /// of your application should be passed to set the proper version for the CLI
    #[cfg(any(feature = "cli", feature = "docs"))]
    #[cfg_attr(feature = "docs", doc(cfg(feature = "cli")))]
    pub fn new_from_flags(version: &str) -> Self {
        // TODO: Support config files too. config-rs and clap don't just work
        // together, so there is no easy way to merge together everything right
        // now. This function is here so we can do that data massaging and
        // merging down the road
        let app = Opts::clap().version(version);
        let opts = Opts::from_clap(&app.get_matches());
        // Copy the addr to avoid a partial move when computing node_ip
        let addr = opts.addr;
        let hostname = opts
            .hostname
            .unwrap_or_else(|| default_hostname().expect("unable to get default hostname"));
        let node_ip = opts.node_ip.unwrap_or_else(|| {
            default_node_ip(&mut hostname.clone(), &addr)
                .expect("unable to get default node IP address")
        });
        let node_name = opts
            .node_name
            .unwrap_or_else(|| sanitize_hostname(&hostname));

        let node_labels = opts
            .node_labels
            .iter()
            .filter_map(|i| split_one_label(i))
            .collect();

        let port = opts.port;

        let data_dir = opts
            .data_dir
            .unwrap_or_else(|| default_data_dir().expect("unable to get default directory"));

        let tls_cert_file = opts
            .tls_cert_file
            .unwrap_or_else(|| default_cert_path(&data_dir));
        let tls_private_key_file = opts
            .tls_private_key_file
            .unwrap_or_else(|| default_key_path(&data_dir));

        let max_pods = opts.max_pods;

        let bootstrap_file = opts.bootstrap_file;

        Config {
            node_ip,
            node_name,
            node_labels,
            hostname,
            data_dir,
            max_pods,
            bootstrap_file,
            server_config: ServerConfig {
                addr,
                port,
                tls_cert_file,
                tls_private_key_file,
            },
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::default_config(
            &"127.0.0.1"
                .parse()
                .expect("Could not parse hardcoded address"),
        )
        .expect("Could not create default config")
    }
}

/// CLI options that can be configured for Kubelet
///
/// These can be parsed from args using `Opts::into_app()`
#[derive(StructOpt, Clone, Debug)]
#[cfg(any(feature = "cli", feature = "docs"))]
#[cfg_attr(feature = "docs", doc(cfg(feature = "cli")))]
#[structopt(
    name = "krustlet",
    about = "A kubelet for running WebAssembly workloads"
)]
pub struct Opts {
    #[structopt(
        short = "a",
        long = "addr",
        default_value = "0.0.0.0",
        env = "KRUSTLET_ADDRESS",
        help = "The address krustlet should listen on"
    )]
    addr: IpAddr,

    #[structopt(
        short = "p",
        long = "port",
        default_value = "3000",
        env = "KRUSTLET_PORT",
        help = "The port krustlet should listen on"
    )]
    port: u16,

    #[structopt(
        long = "max-pods",
        default_value = "110",
        env = "MAX_PODS",
        help = "The maximum pods for this kubelet (reported to apiserver)"
    )]
    max_pods: u16,

    #[structopt(
        long = "tls-cert-file",
        env = "TLS_CERT_FILE",
        help = "The path to kubelet TLS certificate. Defaults to $KRUSTLET_DATA_DIR/config/krustlet.crt"
    )]
    tls_cert_file: Option<PathBuf>,

    #[structopt(
        long = "tls-private-key-file",
        env = "TLS_PRIVATE_KEY_FILE",
        help = "The path to kubelet TLS key. Defaults to $KRUSTLET_DATA_DIR/config/krustlet.key"
    )]
    tls_private_key_file: Option<PathBuf>,

    #[structopt(
        short = "n",
        long = "node-ip",
        env = "KRUSTLET_NODE_IP",
        help = "The IP address of the node registered with the Kubernetes master. Defaults to the IP address of the node name in DNS as a best effort try at a default"
    )]
    node_ip: Option<IpAddr>,

    #[structopt(
        long = "node-labels",
        env = "NODE_LABELS",
        use_delimiter = true,
        help = "Labels to add when registering the node in the cluster.
        Labels must be key=value pairs separated by ','.
        Labels in the 'kubernetes.io' namespace must begin with an allowed prefix
        (kubelet.kubernetes.io, node.kubernetes.io) or be in the specifically allowed set
        (beta.kubernetes.io/arch, beta.kubernetes.io/instance-type, beta.kubernetes.io/os,
        failure-domain.beta.kubernetes.io/region, failure-domain.beta.kubernetes.io/zone,
        failure-domain.kubernetes.io/region, failure-domain.kubernetes.io/zone,
        kubernetes.io/arch, kubernetes.io/hostname, kubernetes.io/instance-type,
        kubernetes.io/os)"
    )]
    node_labels: Vec<String>,

    #[structopt(
        long = "hostname",
        env = "KRUSTLET_HOSTNAME",
        help = "The hostname for this node, defaults to the hostname of this machine"
    )]
    hostname: Option<String>,

    #[structopt(
        long = "node-name",
        env = "KRUSTLET_NODE_NAME",
        help = "The name for this node in Kubernetes, defaults to the hostname of this machine"
    )]
    node_name: Option<String>,

    #[structopt(
        long = "data-dir",
        env = "KRUSTLET_DATA_DIR",
        help = "The data path (logs, container images, etc) for krustlet storage. Defaults to $HOME/.krustlet"
    )]
    data_dir: Option<PathBuf>,

    #[structopt(
        long = "bootstrap-file",
        env = "KRUSTLET_BOOTSTRAP_FILE",
        help = "The path to the bootstrap config",
        default_value = BOOTSTRAP_FILE
    )]
    bootstrap_file: PathBuf,
}

fn default_hostname() -> anyhow::Result<String> {
    Ok(hostname::get()?
        .into_string()
        .map_err(|_| anyhow::anyhow!("invalid utf-8 hostname string"))?)
}

fn default_data_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Unable to get home directory"))?
        .join(".krustlet"))
}

// Some hostnames (particularly local ones) can have uppercase letters, which is
// disallowed by the DNS spec used in kubernetes naming. This sanitizes those
// names
fn sanitize_hostname(hostname: &str) -> String {
    // TODO: Are there other sanitation steps we should do here?
    hostname.to_lowercase()
}

// Attempt to get the node IP address in the following order (this follows the
// same pattern as the Kubernetes kubelet):
// 1. Lookup the IP from node name by DNS
// 2. Try to get the IP from the network interface used as default gateway
//    (unimplemented for now because it doesn't work across platforms)
fn default_node_ip(hostname: &mut String, preferred_ip_family: &IpAddr) -> anyhow::Result<IpAddr> {
    // NOTE: As of right now, we don't have cloud providers. In the future if
    // that is the case, we will need to add logic for looking up the IP and
    // hostname using the cloud provider as they do in the kubelet
    // To use the local resolver, we need to add a port to the hostname. Doesn't
    // matter which one, it just needs to be a valid socket address
    hostname.push_str(":80");
    Ok(hostname
        .to_socket_addrs()?
        .find(|i| {
            !i.ip().is_loopback()
                && !i.ip().is_multicast()
                && !i.ip().is_unspecified()
                && is_same_ip_family(&i.ip(), preferred_ip_family)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unable to find default IP address for node. Please specify a node IP manually"
            )
        })?
        .ip())
}

fn default_key_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("config/krustlet.key")
}

fn default_cert_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("config/krustlet.crt")
}

fn is_same_ip_family(first: &IpAddr, second: &IpAddr) -> bool {
    match first {
        IpAddr::V4(_) => second.is_ipv4(),
        IpAddr::V6(_) => second.is_ipv6(),
    }
}

fn split_one_label(in_string: &str) -> Option<(String, String)> {
    let mut splitter = in_string.splitn(2, '=');

    match splitter.next() {
        Some("") | None => None,
        Some(key) => match splitter.next() {
            Some(val) => Some((key.to_string(), val.to_string())),
            None => Some((key.to_string(), String::new())),
        },
    }
}
