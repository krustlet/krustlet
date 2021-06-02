//! Helpers for parsing resource names and types. Checks that they follow the expected resource formatting
//! explained in [this Kubernetes proposal](https://github.com/kubernetes/community/blob/master/contributors/design-proposals/scheduling/resources.md#the-kubernetes-resource-model).
// TODO: decide whether this should go under container or some container manager folder
use k8s_openapi::api::core::v1::Container as KubeContainer;
use k8s_openapi::api::core::v1::ResourceRequirements;
use regex::{Regex, RegexBuilder};
use tracing::{debug, error, info};

const QUALIFIED_NAME_MAX_LENGTH: usize = 63;
const QUALIFIED_NAME_CHAR_FMT: &str = "[A-Za-z0-9]";
const QUALIFIED_NAME_EXT_CHAR_FMT: &str = "[-A-Za-z0-9_.]";
const QUALIFIED_NAME_FMT: &str = concat!("(", "[A-Za-z0-9]", "[-A-Za-z0-9_.]", "*)?", "[A-Za-z0-9]");
const QUALIFIED_NAME_ERR_MSG: &str = "must consist of alphanumeric characters, '-', '_' or '.', and must start and end with an alphanumeric character";

// DNS1123SubdomainMaxLength is a subdomain's max length in DNS (RFC 1123)
const DNS_1123_SUBDOMAIN_MAX_LEN: usize = 253;
const DNS_1123_LABEL_FMT: &str = "[a-z0-9]([-a-z0-9]*[a-z0-9])?";
const DNS_1123_LABEL_ERR_MSG: &str = "a lowercase RFC 1123 label must consist of lower case alphanumeric characters or '-', and must start and end with an alphanumeric character";
const DNS_1123_SUBDOMAIN_FMT: &str = concat!("[a-z0-9]([-a-z0-9]*[a-z0-9])?", "(\\.",  "[a-z0-9]([-a-z0-9]*[a-z0-9])?", ")*"); //  DNS_1123_LABEL_FMT + "(\\." + DNS_1123_LABEL_FMT + ")*" 
const DNS_1123_SUBDOMAIN_ERR_MSG: &str = "a lowercase RFC 1123 subdomain must consist of lower case alphanumeric characters, '-' or '.', and must start and end with an alphanumeric character";

/// RESOURCE_DEFAULT_NAMESPACE_PREFIX is the default namespace prefix.
const RESOURCE_DEFAULT_NAMESPACE_PREFIX: &str = "kubernetes.io/";
/// Default resource requests prefix
const DEFAULT_RESOURCE_REQUESTS_PREFIX: &str = "requests.";

// Taken from oci_distribution::regexp (which is private)
pub fn must_compile(r: &str) -> Regex {
    RegexBuilder::new(r)
        .size_limit(10 * (1 << 21))
        .build()
        .unwrap()
}

/// is_extended_resource_name returns true if:
/// 1. the resource name is not in the default namespace;
/// 2. resource name does not have "requests." prefix,
/// to avoid confusion with the convention in quota
/// 3. it satisfies the rules in IsQualifiedName() after converted into quota resource name
/// Following implementation from https://github.com/kubernetes/kubernetes/blob/v1.21.1/pkg/apis/core/helper/helpers.go#L174
pub fn is_extended_resource_name(name: &str) -> bool {
    if is_native_resource(name) || name.starts_with(DEFAULT_RESOURCE_REQUESTS_PREFIX) {
        false
    } else {
        let quota_resource_name = format!("{}{}", DEFAULT_RESOURCE_REQUESTS_PREFIX, name);
        match is_qualified_name(&quota_resource_name) {
            Ok(_) => true,
            Err(e) => {
                println!("name {} does not qualify as an extended resource name due to {}", name, e);
                false
            }
        }
    }
}


/// is_native_resource returns true if the resource name is in the
/// *kubernetes.io/ namespace. Partially-qualified (unprefixed) names are
/// implicitly in the kubernetes.io/ namespace.
fn is_native_resource(name: &str) -> bool {
	return !name.contains("/") ||
		name.contains(RESOURCE_DEFAULT_NAMESPACE_PREFIX)
}

/// is_qualified_name tests whether the value passed is what Kubernetes calls a "qualified name".  
/// A fully-qualified resource typename is constructed from a DNS-style subdomain, 
/// followed by a slash /, followed by a name. 
/// This is a format used in various places throughout the system. 
/// Returns first improper formatting error hit.
fn is_qualified_name(name: &str) -> anyhow::Result<()> {
    // List of error strings to be compiled into one error
    let parts: Vec<&str> = name.split('/').collect();
    let mut name = "";
    match parts.len() {
        1 => name = parts[0],
        2 => {
            let prefix = parts[0];
            name = parts[1];
            if prefix.len() == 0 {
                return Err(anyhow::Error::msg("prefix part must not be empty")); 
            } 
            is_dns_1123_subdomain(prefix)?;
        },
        _ => { return Err(anyhow::Error::msg("a qualified name was expected with an optional DNS subdomain prefix and '/' (e.g. 'example.com/MyName')"));},
    }

    if name.len() > QUALIFIED_NAME_MAX_LENGTH {
        return Err(anyhow::format_err!("expected qualified name to be no longer than {} characters", QUALIFIED_NAME_MAX_LENGTH));
    }

    if !must_compile(&format!("^{}$", QUALIFIED_NAME_FMT)).is_match(name) {
        return Err(anyhow::format_err!("qualified name not properly formatted ... {}", QUALIFIED_NAME_ERR_MSG));
    }

    Ok(())
}

/// is_dns_1123_subdomain tests for a string that conforms to the definition of a
/// subdomain in DNS (RFC 1123).
fn is_dns_1123_subdomain(value: &str) -> anyhow::Result<()>  {
    if value.len() > DNS_1123_SUBDOMAIN_MAX_LEN {
        Err(anyhow::format_err!("DNS subdomain cannot be more than {} characters", DNS_1123_SUBDOMAIN_MAX_LEN))
    } else if !must_compile(&format!("^{}$", DNS_1123_SUBDOMAIN_FMT)).is_match(value) {
        // TODO: give more verbose error message?
        // https://sourcegraph.com/github.com/kubernetes/kubernetes@b496238dd65d86c65183ac7ffa128c5de46705b4/-/blob/staging/src/k8s.io/apimachinery/pkg/util/validation/validation.go?subtree=true#L214:3
        Err(anyhow::format_err!("dns subdomain not properly formatted ... {}", DNS_1123_LABEL_ERR_MSG))
    } else {
        Ok(())    
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_extended_resource_name() {
        let passing_name = "example.com/dongle";
        assert!(is_extended_resource_name(passing_name));
        let default_namespace_prefixed_name = "kubernetes.io/memory";
        assert!(!is_extended_resource_name(default_namespace_prefixed_name));
        let requests_prefixed_name = "requests.example.com/dongle";
        assert!(!is_extended_resource_name(requests_prefixed_name));
        let no_prefix_name = "dongle";
        assert!(!is_extended_resource_name(no_prefix_name));
    }
}