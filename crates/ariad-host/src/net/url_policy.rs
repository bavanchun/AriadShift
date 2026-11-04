//! URL parsing, scheme enforcement, and safe redirect validation.

use url::Url;

use super::error::NetError;

/// URL policy rules for outbound connections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UrlPolicy {
    /// Whether only HTTPS URLs are allowed.
    pub https_only: bool,
    /// Whether loopback HTTP connections are allowed (for tests/development).
    pub allow_loopback: bool,
    /// Explicitly allowed ports beyond default 443 (HTTPS) and 80 (HTTP).
    pub allowed_ports: Vec<u16>,
}

impl Default for UrlPolicy {
    fn default() -> Self {
        Self {
            https_only: true,
            allow_loopback: false,
            allowed_ports: Vec::new(),
        }
    }
}

/// Validates that a parsed URL complies with the security policy.
pub fn validate_url(url: &Url, policy: &UrlPolicy) -> Result<(), NetError> {
    // Scheme checks
    match url.scheme() {
        "https" => {}
        "http" => {
            if policy.https_only {
                return Err(NetError::UnsupportedScheme {
                    scheme: "http".to_owned(),
                });
            }
        }
        other => {
            return Err(NetError::UnsupportedScheme {
                scheme: other.to_owned(),
            });
        }
    }

    // Credentials checks: userinfo is forbidden
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NetError::CredentialsNotAllowed);
    }

    // Port check: default 443 (https) and 80 (http). Non-standard ports must be allowed by policy.
    let port = url
        .port_or_known_default()
        .unwrap_or(if url.scheme() == "https" { 443 } else { 80 });
    let is_standard = match url.scheme() {
        "https" => port == 443,
        "http" => port == 80,
        _ => false,
    };
    if !is_standard && !policy.allow_loopback && !policy.allowed_ports.contains(&port) {
        return Err(NetError::PortNotAllowed { port });
    }

    // Host check
    let host = url
        .host_str()
        .ok_or_else(|| NetError::InvalidUrl("missing host".to_owned()))?;

    if host.is_empty() {
        return Err(NetError::InvalidUrl("empty host".to_owned()));
    }

    // Refuse control characters or NUL in host/path
    if url.as_str().bytes().any(|b| b.is_ascii_control()) {
        return Err(NetError::InvalidUrl(
            "URL contains control characters".to_owned(),
        ));
    }

    Ok(())
}

/// Resolves and validates a redirect target against the current URL and policy.
pub fn validate_redirect(
    current: &Url,
    location: &str,
    policy: &UrlPolicy,
) -> Result<Url, NetError> {
    if location.is_empty() {
        return Err(NetError::InvalidRedirect(
            "empty Location header".to_owned(),
        ));
    }

    let target = current
        .join(location)
        .map_err(|e| NetError::InvalidRedirect(format!("malformed Location '{location}': {e}")))?;

    // Strict downgrade prevention: HTTPS -> HTTP is always rejected
    if current.scheme() == "https" && target.scheme() == "http" {
        return Err(NetError::DowngradeRefused(target.to_string()));
    }

    validate_url(&target, policy)?;

    Ok(target)
}
