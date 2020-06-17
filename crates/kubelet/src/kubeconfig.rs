use std::env;
use std::path::PathBuf;

use dirs::home_dir;

pub const KUBECONFIG: &str = "KUBECONFIG";

/// Search the kubeconfig file
pub(crate) fn exists() -> bool {
    path().unwrap_or_default().exists()
}

/// Returns kubeconfig path from specified environment variable.
fn path() -> Option<PathBuf> {
    env::var_os(KUBECONFIG)
        .map(PathBuf::from)
        .or_else(default_path)
}

/// Returns kubeconfig path from `$HOME/.kube/config`.
fn default_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".kube").join("config"))
}
