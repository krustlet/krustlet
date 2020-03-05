use clap::derive::Clap;
use std::net::IpAddr;
use std::net::ToSocketAddrs;

/// The configuration needed for a kubelet to run properly. This can be
/// configured manually in your code or if you are exposing a CLI, use the
/// [get_from_flags method](get_from_flags). Use
/// [default_config](Config::default_config) to generate a config with all of
/// the default values set.
#[derive(Clone)]
pub struct Config {
    pub addr: IpAddr,

    pub port: u16,

    pub node_ip: IpAddr,
    pub hostname: String,

    pub node_name: String,

    pub(crate) arch: String,
}

impl Config {
    /// Returns a Config object set with all of the defaults. Useful for cases
    /// when you don't want to set most of the values yourself. The
    /// preferred_ip_family argument takes an IpAddr that is either V4 or V6 to
    /// indicate the preferred IP family to use for defaults
    pub fn default_config(preferred_ip_family: &IpAddr) -> Result<Self, failure::Error> {
        let hostname = default_hostname()?;
        Ok(Config {
            addr: match preferred_ip_family {
                // Just unwrap these because they are programmer error if they
                // don't parse
                IpAddr::V4(_) => "0.0.0.0".parse().unwrap(),
                IpAddr::V6(_) => "::".parse().unwrap(),
            },
            port: 3000,
            node_ip: default_node_ip(&hostname, preferred_ip_family)?,
            node_name: sanitize_hostname(&hostname),
            hostname,
            arch: String::default(),
        })
    }
}

// Opts contains the values that can be configured for kubelet
#[derive(clap::Clap, Clone, Debug)]
pub struct Opts {
    #[clap(
        short = "a",
        long = "addr",
        default_value = "0.0.0.0",
        env = "KRUSTLET_ADDRESS",
        help = "The address krustlet should listen on"
    )]
    addr: IpAddr,

    #[clap(
        short = "p",
        long = "port",
        default_value = "3000",
        env = "KRUSTLET_PORT",
        help = "The port krustlet should listen on"
    )]
    port: u16,

    #[clap(
        short = "n",
        long = "node-ip",
        env = "KRUSTLET_NODE_IP",
        help = "The IP address of the node registered with the Kubernetes master. Defaults to the IP address of the node name in DNS and then IP address of the network interface used as the default gateway"
    )]
    node_ip: Option<IpAddr>,

    #[clap(
        long = "hostname",
        env = "KRUSTLET_HOSTNAME",
        help = "The hostname for this node, defaults to the hostname of this machine"
    )]
    hostname: Option<String>,

    #[clap(
        long = "node-name",
        env = "KRUSTLET_NODE_NAME",
        help = "The name for this node in Kubernetes, defaults to the hostname of this machine"
    )]
    node_name: Option<String>,
}

/// Parses all command line flags and sets the proper defaults
pub fn get_from_flags() -> Config {
    // TODO: Support config files too. config-rs and clap don't just work
    // together, so there is no easy way to merge together everything right
    // now. This function is here so we can do that data massaging and
    // merging down the road
    let opts = Opts::parse();
    // Copy the addr to avoid a partial move when computing node_ip
    let addr = opts.addr;
    let hostname = opts
        .hostname
        .unwrap_or_else(|| default_hostname().expect("unable to get default hostname"));
    let node_ip = opts.node_ip.unwrap_or_else(|| {
        default_node_ip(&hostname, &addr).expect("unable to get default node IP address")
    });
    Config {
        addr,
        port: 3000,
        node_ip,
        node_name: sanitize_hostname(&hostname),
        hostname,
        arch: String::default(),
    }
}

fn default_hostname() -> Result<String, failure::Error> {
    Ok(hostname::get()?
        .into_string()
        .map_err(|_| format_err!("invalid hostname string"))?)
}

// Some hostnames (particularly local ones) can have uppercase letters, which is
// disallowed by the DNS spec used in kubernetes naming. This sanitizes those
// names
fn sanitize_hostname(hostname: &str) -> String {
    // TODO: Are there other sanitation steps we should do here?
    hostname.to_owned().to_lowercase()
}

// Attempt to get the node IP address in the following order (this follows the
// same pattern as the Kubernetes kubelet):
// 1. Lookup the IP from node name by DNS
// 2. Try to get the IP from the network interface used as default gateway
//    (unimplemented for now because it doesn't work across platforms)
fn default_node_ip(hostname: &str, preferred_ip_family: &IpAddr) -> Result<IpAddr, failure::Error> {
    // NOTE: As of right now, we don't have cloud providers. In the future if
    // that is the case, we will need to add logic for looking up the IP and
    // hostname using the cloud provider as they do in the kubelet
    // To use the local resolver, we need to add a port to the hostname. Doesn't
    // matter which one, it just needs to be a valid socket address
    let mut with_port = hostname.to_owned();
    with_port.push_str(":80");
    Ok(with_port
        .to_socket_addrs()?
        .find(|i| !&i.ip().is_loopback() && is_same_ip_family(&i.ip(), preferred_ip_family))
        .ok_or_else(|| {
            format_err!(
                "unable to find default IP address for node. Please specify a node IP manually"
            )
        })?
        .ip())
}

fn is_same_ip_family(first: &IpAddr, second: &IpAddr) -> bool {
    match first {
        IpAddr::V4(_) => second.is_ipv4(),
        IpAddr::V6(_) => second.is_ipv6(),
    }
}
