//! Host broker and secret management for digital signatures.
//!
//! Enforces:
//! 1. Binary secret handle staged to `<work_dir>/cert.p12` with mode 0600 on unix,
//!    strictly bounded to 10 MiB, cleaned up unconditionally on exit.
//! 2. TSA & revocation broker using [`crate::net::Client`] with DNS pinning,
//!    public IP verification, HTTPS only, and bounded response size and timeout.

use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::Engine as _;
use serde_json::Value;
use url::Url;

use crate::{
    net::{Client, NetError},
    tools::{run::ToolError, secrets::SecretBag},
    workspace::Workspace,
};

/// Maximum allowed certificate file size for staging (10 MiB).
pub const MAX_CERT_BYTES: u64 = 10 * 1024 * 1024;

/// RAII guard ensuring temporary private secret files are cleaned up.
pub struct SecretFileGuard(pub Option<PathBuf>);

impl Drop for SecretFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn resolve_cert_bytes(input: &str) -> Result<Vec<u8>, ToolError> {
    let trimmed = input.trim();

    // 1. Prefer base64 encoded payload (standard transport for CLI and MCP secrets)
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(trimmed) {
        match decoded.len() as u64 {
            0 => {}
            1..=MAX_CERT_BYTES => return Ok(decoded),
            _ => return Err(ToolError::LimitExceeded),
        }
    }

    // 2. Allow-list approach for explicit file path input
    let path = Path::new(trimmed);
    if path.is_file() {
        let canonical = fs::canonicalize(path).map_err(|e| {
            ToolError::InvalidParams(format!("cannot resolve certificate file: {e}"))
        })?;

        let canonical_str = canonical.to_string_lossy();
        if canonical_str.starts_with("/proc")
            || canonical_str.starts_with("/sys")
            || canonical_str.starts_with("/dev")
            || canonical_str.starts_with("/etc")
            || canonical_str.starts_with("/var")
            || canonical_str.starts_with("/run")
            || canonical_str.contains("/.ssh")
            || canonical_str.contains("/.gnupg")
            || canonical_str.contains("/.aws")
            || canonical_str.contains("/.config")
        {
            return Err(ToolError::InvalidParams(
                "certificate path points to a forbidden system or sensitive directory".to_owned(),
            ));
        }

        let meta = fs::metadata(&canonical)
            .map_err(|e| ToolError::InvalidParams(format!("cannot read certificate file: {e}")))?;

        if !meta.file_type().is_file() || meta.len() == 0 {
            return Err(ToolError::InvalidParams(
                "certificate file must be a non-empty regular file".to_owned(),
            ));
        }
        if meta.len() > MAX_CERT_BYTES {
            return Err(ToolError::LimitExceeded);
        }

        return fs::read(&canonical)
            .map_err(|e| ToolError::InvalidParams(format!("cannot read certificate file: {e}")));
    }

    Err(ToolError::InvalidParams(
        "certificate parameter must be base64-encoded data or an explicit certificate file path"
            .to_owned(),
    ))
}

/// Stages a PKCS#12 certificate file or binary data into `<work_dir>/cert.p12` (0600).
pub fn stage_binary_certificate(
    workspace: &Workspace,
    params: &Value,
    secrets: &SecretBag,
) -> Result<SecretFileGuard, ToolError> {
    let work_dir = workspace.work_dir();
    let cert_target = work_dir.join("cert.p12");

    // 1. Check if supplied in SecretBag
    if let Some(cert_str) = secrets.get("certificate") {
        let bytes = resolve_cert_bytes(cert_str)?;
        return write_private_cert_bytes(&cert_target, &bytes);
    }

    // 2. Check if supplied in params
    if let Some(cert_val) = params.get("certificate").and_then(Value::as_str) {
        let bytes = resolve_cert_bytes(cert_val)?;
        return write_private_cert_bytes(&cert_target, &bytes);
    }

    Ok(SecretFileGuard(None))
}

fn write_private_cert_bytes(target: &Path, bytes: &[u8]) -> Result<SecretFileGuard, ToolError> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(target)
            .map_err(|e| {
                ToolError::EngineFailure(format!("failed to stage private certificate: {e}"))
            })?;

        file.write_all(bytes).map_err(|e| {
            ToolError::EngineFailure(format!("failed to write private certificate: {e}"))
        })?;
    }

    #[cfg(not(unix))]
    {
        fs::write(target, bytes).map_err(|e| {
            ToolError::EngineFailure(format!("failed to write private certificate: {e}"))
        })?;
    }

    Ok(SecretFileGuard(Some(target.to_path_buf())))
}

thread_local! {
    static TEST_TSA_CLIENT: std::cell::RefCell<Option<Client>> = const { std::cell::RefCell::new(None) };
}

/// Sets a thread-local test TSA client for loopback testing.
#[doc(hidden)]
pub fn with_test_tsa_client<F, R>(client: Client, f: F) -> R
where
    F: FnOnce() -> R,
{
    TEST_TSA_CLIENT.with(|c| *c.borrow_mut() = Some(client));
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    TEST_TSA_CLIENT.with(|c| *c.borrow_mut() = None);
    match res {
        Ok(r) => r,
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// Brokered TSA timestamp fetch using SSRF-safe network client.
pub fn broker_tsa_timestamp(
    workspace: &Workspace,
    tsa_url_str: &str,
    request_data: &[u8],
    client: Option<&Client>,
) -> Result<SecretFileGuard, ToolError> {
    let url = Url::parse(tsa_url_str)
        .map_err(|_| ToolError::InvalidParams("invalid TSA URL".to_owned()))?;

    let is_cloud = std::env::var("ASHIFT_CLOUD").is_ok()
        || std::env::var("ARIAD_CLOUD").is_ok()
        || std::env::var("ASHIFT_DEPLOYMENT").as_deref() == Ok("cloud");

    // In cloud environments or when allowlist is configured, verify destination domain
    if let Ok(allowlist) = std::env::var("ASHIFT_TSA_ALLOWLIST") {
        let allowed_entries: Vec<&str> = allowlist
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let matched = allowed_entries.iter().any(|&item| {
            if let Ok(item_url) = Url::parse(item) {
                item_url.scheme() == url.scheme()
                    && item_url
                        .host_str()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case(url.host_str().unwrap_or_default())
                    && item_url.port_or_known_default() == url.port_or_known_default()
            } else {
                let (h, p) = if let Some((h, p_str)) = item.split_once(':') {
                    (h, p_str.parse::<u16>().ok())
                } else {
                    (item, None)
                };
                h.eq_ignore_ascii_case(url.host_str().unwrap_or_default())
                    && p.is_none_or(|port| Some(port) == url.port_or_known_default())
            }
        });
        if !matched {
            return Err(ToolError::EngineFailure(
                "TSA URL is not in the allowed TSA list".to_owned(),
            ));
        }
    } else if is_cloud {
        return Err(ToolError::EngineFailure(
            "TSA requests are disabled in cloud unless an allowlist is configured".to_owned(),
        ));
    }

    let seam_client = TEST_TSA_CLIENT.with(|c| c.borrow().clone());

    let default_client;
    let net_client = match client.or(seam_client.as_ref()) {
        Some(c) => c,
        None => {
            default_client = Client::builder().build().map_err(|_| {
                ToolError::EngineFailure("failed to initialize safe network client".to_owned())
            })?;
            &default_client
        }
    };

    let response = net_client
        .post(url, request_data.to_vec(), "application/timestamp-query")
        .map_err(|e| match e {
            NetError::NonPublicAddress { .. } => {
                ToolError::EngineFailure("TSA URL resolves to forbidden address".to_owned())
            }
            NetError::UnsupportedScheme { .. } => {
                ToolError::InvalidParams("TSA URL must use HTTPS".to_owned())
            }
            NetError::Timeout => ToolError::EngineFailure("TSA request timed out".to_owned()),
            NetError::ResponseTooLarge { .. } => ToolError::LimitExceeded,
            _ => ToolError::EngineFailure("TSA request failed".to_owned()),
        })?;

    if response.status != 200 {
        return Err(ToolError::EngineFailure(format!(
            "TSA server responded with HTTP status {}",
            response.status
        )));
    }

    let target = workspace.work_dir().join("tsa_resp.bin");
    write_private_cert_bytes(&target, &response.body)
}
