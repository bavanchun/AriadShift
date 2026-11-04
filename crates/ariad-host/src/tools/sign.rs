//! Host broker and secret management for digital signatures (D6, D7).
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
        return write_private_cert_bytes(&cert_target, cert_str.as_bytes());
    }

    // 2. Check if supplied in params
    if let Some(cert_val) = params.get("certificate").and_then(Value::as_str) {
        let path = Path::new(cert_val);
        if path.is_file() {
            let meta = fs::metadata(path).map_err(|e| {
                ToolError::InvalidParams(format!("cannot read certificate file: {e}"))
            })?;
            if meta.len() > MAX_CERT_BYTES {
                return Err(ToolError::LimitExceeded);
            }
            let bytes = fs::read(path).map_err(|e| {
                ToolError::InvalidParams(format!("cannot read certificate file: {e}"))
            })?;
            return write_private_cert_bytes(&cert_target, &bytes);
        }

        // Could be base64 data
        if let Ok(bytes) = base64_decode(cert_val) {
            if bytes.len() as u64 > MAX_CERT_BYTES {
                return Err(ToolError::LimitExceeded);
            }
            return write_private_cert_bytes(&cert_target, &bytes);
        }
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

/// Brokered TSA timestamp fetch using SSRF-safe client (D7).
pub fn broker_tsa_timestamp(
    workspace: &Workspace,
    tsa_url_str: &str,
    request_data: &[u8],
    client: Option<&Client>,
) -> Result<SecretFileGuard, ToolError> {
    let url = Url::parse(tsa_url_str)
        .map_err(|e| ToolError::InvalidParams(format!("invalid TSA URL '{tsa_url_str}': {e}")))?;

    let default_client;
    let net_client = match client {
        Some(c) => c,
        None => {
            default_client = Client::new().map_err(|e| {
                ToolError::EngineFailure(format!("failed to initialize safe network client: {e}"))
            })?;
            &default_client
        }
    };

    let response = net_client
        .post(url, request_data.to_vec(), "application/timestamp-query")
        .map_err(|e| match e {
            NetError::NonPublicAddress { ip, .. } => ToolError::EngineFailure(format!(
                "TSA URL resolves to forbidden non-public IP: {ip}"
            )),
            NetError::UnsupportedScheme { scheme } => ToolError::InvalidParams(format!(
                "TSA URL has unsupported scheme '{scheme}', HTTPS required"
            )),
            NetError::Timeout => ToolError::EngineFailure("TSA request timed out".to_owned()),
            NetError::ResponseTooLarge { .. } => ToolError::LimitExceeded,
            other => ToolError::EngineFailure(format!("TSA request failed: {other}")),
        })?;

    if response.status != 200 {
        return Err(ToolError::EngineFailure(format!(
            "TSA server responded with HTTP status {}",
            response.status
        )));
    }

    let target = workspace.work_dir().join("tsa_response.bin");
    write_private_cert_bytes(&target, &response.body)
}

fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    // Quick base64 check without pulling new crates
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if !cleaned.len().is_multiple_of(4) {
        return Err(());
    }
    // Simple custom standard base64 decoder
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let decode_byte = |b: u8| -> Option<u8> {
        if b == b'=' {
            return Some(0);
        }
        TABLE.iter().position(|&x| x == b).map(|p| p as u8)
    };

    let bytes = cleaned.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() != 4 {
            return Err(());
        }
        let b0 = decode_byte(chunk[0]).ok_or(())?;
        let b1 = decode_byte(chunk[1]).ok_or(())?;
        let b2 = decode_byte(chunk[2]).ok_or(())?;
        let b3 = decode_byte(chunk[3]).ok_or(())?;

        out.push((b0 << 2) | (b1 >> 4));
        if chunk[2] != b'=' {
            out.push((b1 << 4) | (b2 >> 2));
        }
        if chunk[3] != b'=' {
            out.push((b2 << 6) | b3);
        }
    }
    Ok(out)
}
