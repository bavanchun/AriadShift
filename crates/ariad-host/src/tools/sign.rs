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
    let path = Path::new(trimmed);
    if path.is_file() {
        let canonical = fs::canonicalize(path).map_err(|e| {
            ToolError::InvalidParams(format!("cannot resolve certificate file: {e}"))
        })?;

        let canonical_str = canonical.to_string_lossy();
        if canonical_str.starts_with("/proc")
            || canonical_str.starts_with("/sys")
            || canonical_str.starts_with("/dev")
            || canonical_str.starts_with("/etc/shadow")
            || canonical_str.starts_with("/etc/master.passwd")
        {
            return Err(ToolError::InvalidParams(
                "certificate path points to a forbidden system location".to_owned(),
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

    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(trimmed) {
        match decoded.len() as u64 {
            0 => {}
            1..=MAX_CERT_BYTES => return Ok(decoded),
            _ => return Err(ToolError::LimitExceeded),
        }
    }

    if trimmed.len() as u64 > MAX_CERT_BYTES {
        return Err(ToolError::LimitExceeded);
    }
    Ok(trimmed.as_bytes().to_vec())
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

/// Brokered TSA timestamp fetch using SSRF-safe network client.
pub fn broker_tsa_timestamp(
    workspace: &Workspace,
    tsa_url_str: &str,
    request_data: &[u8],
    client: Option<&Client>,
) -> Result<SecretFileGuard, ToolError> {
    let url = Url::parse(tsa_url_str)
        .map_err(|_| ToolError::InvalidParams("invalid TSA URL".to_owned()))?;

    // In cloud environments or when allowlist is configured, verify destination domain
    if let Ok(allowlist) = std::env::var("ASHIFT_TSA_ALLOWLIST") {
        let allowed: Vec<&str> = allowlist.split(',').map(str::trim).collect();
        let host = url.host_str().unwrap_or_default();
        if !allowed
            .iter()
            .any(|&item| item.eq_ignore_ascii_case(host) || url.as_str().starts_with(item))
        {
            return Err(ToolError::EngineFailure(
                "TSA URL is not in the allowed TSA list".to_owned(),
            ));
        }
    }

    let default_client;
    let net_client = match client {
        Some(c) => c,
        None => {
            let mut builder = Client::builder();
            if std::env::var_os("ASHIFT_NET_ALLOW_LOOPBACK").is_some() || cfg!(test) {
                builder = builder.allow_loopback(true);
                let mut ports = vec![80, 443, 8080, 8443];
                ports.extend(1024..=65535);
                builder = builder.allowed_ports(ports);
            }
            default_client = builder.build().map_err(|_| {
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
