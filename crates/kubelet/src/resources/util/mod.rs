//! Helpers for parsing resource names and types. Checks that they follow the expected resource
//! formatting explained in [this Kubernetes
//! proposal](https://github.com/kubernetes/community/blob/master/contributors/design-proposals/scheduling/resources.md#the-kubernetes-resource-model).
use regex::{Regex, RegexBuilder};
use tracing::trace;

const QUALIFIED_NAME_MAX_LENGTH: usize = 63;
// QUALIFIED_NAME_FMT = "("" + QUALIFIED_NAME_CHAR_FMT + "*)?"" + QUALIFIED_NAME_EXT_CHAR_FMT +
// QUALIFIED_NAME_CHAR_FMT
const QUALIFIED_NAME_FMT: &str =
    concat!("(", "[A-Za-z0-9]", "[-A-Za-z0-9_.]", "*)?", "[A-Za-z0-9]");
const QUALIFIED_NAME_ERR_MSG: &str = "must consist of alphanumeric characters, '-', '_' or '.', and must start and end with an alphanumeric character";

// DNS1123SubdomainMaxLength is a subdomain's max length in DNS (RFC 1123)
const DNS_1123_SUBDOMAIN_MAX_LEN: usize = 253;
const DNS_1123_LABEL_ERR_MSG: &str = "a lowercase RFC 1123 label must consist of lower case alphanumeric characters or '-', and must start and end with an alphanumeric character";
// DNS_1123_SUBDOMAIN_FMT = DNS_1123_LABEL_FMT + "(\\." + DNS_1123_LABEL_FMT + ")*"
const DNS_1123_SUBDOMAIN_FMT: &str = concat!(
    "[a-z0-9]([-a-z0-9]*[a-z0-9])?",
    "(\\.",
    "[a-z0-9]([-a-z0-9]*[a-z0-9])?",
    ")*"
);

/// RESOURCE_DEFAULT_NAMESPACE_PREFIX is the default namespace prefix.
const RESOURCE_DEFAULT_NAMESPACE_PREFIX: &str = "kubernetes.io/";
/// Default resource requests prefix
const DEFAULT_RESOURCE_REQUESTS_PREFIX: &str = "requests.";

/// Creates a new regex builder with the input pattern. Throws error if the pattern is invalid.
/// Taken from oci_distribution::regexp (which is private)
pub fn must_compile(r: &str) -> Regex {
    RegexBuilder::new(r)
        .size_limit(10 * (1 << 21))
        .build()
        .unwrap()
}

/// Returns true if:
/// 1. the resource name is not in the default namespace;
/// 2. resource name does not have "requests." prefix, to avoid confusion with the convention in
///    quota
/// 3. it satisfies the rules in IsQualifiedName() after converted into quota resource name
///    Following implementation from
///    https://github.com/kubernetes/kubernetes/blob/v1.21.1/pkg/apis/core/helper/helpers.go#L174
pub fn is_extended_resource_name(name: &str) -> bool {
    if is_native_resource(name) || name.starts_with(DEFAULT_RESOURCE_REQUESTS_PREFIX) {
        false
    } else {
        let quota_resource_name = format!("{}{}", DEFAULT_RESOURCE_REQUESTS_PREFIX, name);
        match is_qualified_name(&quota_resource_name) {
            Ok(_) => true,
            Err(e) => {
                trace!(
                    "name {} does not qualify as an extended resource name due to {}",
                    name,
                    e
                );
                false
            }
        }
    }
}

/// Returns true if the resource name is in the *kubernetes.io/ namespace. Partially-qualified
/// (unprefixed) names are implicitly in the kubernetes.io/ namespace.
fn is_native_resource(name: &str) -> bool {
    !name.contains('/') || name.contains(RESOURCE_DEFAULT_NAMESPACE_PREFIX)
}

/// Tests whether the value passed is what Kubernetes calls a "qualified name".  
/// A fully-qualified resource typename is constructed from a DNS-style subdomain, followed by a
/// slash /, followed by a name. This is a format used in various places throughout the system.
/// Returns first improper formatting error hit.
fn is_qualified_name(name: &str) -> anyhow::Result<()> {
    // List of error strings to be compiled into one error
    let parts: Vec<&str> = name.split('/').collect();
    let name;
    match parts.len() {
        1 => name = parts[0],
        2 => {
            let prefix = parts[0];
            name = parts[1];
            if prefix.is_empty() {
                return Err(anyhow::Error::msg("prefix part must not be empty"));
            }
            is_dns_1123_subdomain(prefix)?;
        }
        _ => {
            return Err(anyhow::Error::msg("a qualified name was expected with an optional DNS subdomain prefix and '/' (e.g. 'example.com/MyName')"));
        }
    }

    if name.len() > QUALIFIED_NAME_MAX_LENGTH {
        return Err(anyhow::format_err!(
            "expected qualified name to be no longer than {} characters",
            QUALIFIED_NAME_MAX_LENGTH
        ));
    }

    if !must_compile(&format!("^{}$", QUALIFIED_NAME_FMT)).is_match(name) {
        return Err(anyhow::format_err!(
            "qualified name not properly formatted ... {}",
            QUALIFIED_NAME_ERR_MSG
        ));
    }

    Ok(())
}

/// Tests for a string that conforms to the definition of a subdomain in DNS (RFC 1123).
fn is_dns_1123_subdomain(value: &str) -> anyhow::Result<()> {
    if value.len() > DNS_1123_SUBDOMAIN_MAX_LEN {
        Err(anyhow::format_err!(
            "DNS subdomain cannot be more than {} characters",
            DNS_1123_SUBDOMAIN_MAX_LEN
        ))
    } else if !must_compile(&format!("^{}$", DNS_1123_SUBDOMAIN_FMT)).is_match(value) {
        Err(anyhow::format_err!(
            "dns subdomain not properly formatted ... {}",
            DNS_1123_LABEL_ERR_MSG
        ))
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
