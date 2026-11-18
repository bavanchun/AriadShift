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
    net::{Client, ClientBuilder, NetError},
    tools::{run::ToolError, secrets::SecretBag},
    workspace::Workspace,
};

/// Maximum allowed certificate file size for staging (10 MiB).
pub const MAX_CERT_BYTES: u64 = 10 * 1024 * 1024;

/// RAII guard ensuring temporary private secret files are cleaned up.
#[derive(Debug)]
pub struct SecretFileGuard(pub Option<PathBuf>);

impl Drop for SecretFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) fn resolve_cert_bytes(input: &str) -> Result<Vec<u8>, ToolError> {
    let trimmed = input.trim();

    // The host strictly rejects filesystem paths. CLI (--secret certificate=@FILE)
    // and MCP (secret_put with base64) must read bytes on their side.
    if trimmed.starts_with('/')
        || trimmed.starts_with('~')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with(r".\")
        || trimmed.starts_with(r"..\")
        || (trimmed.len() >= 3
            && trimmed.as_bytes()[1] == b':'
            && (trimmed.as_bytes()[2] == b'\\' || trimmed.as_bytes()[2] == b'/'))
    {
        return Err(ToolError::InvalidParams(
            "certificate secret must be base64-encoded data, not a filesystem path; pass file contents via CLI --secret certificate=@<path> or MCP base64 secret_put".to_owned(),
        ));
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| {
            ToolError::InvalidParams(
                "certificate parameter must be base64-encoded data (file paths are not accepted by the host)".to_owned(),
            )
        })?;

    match decoded.len() as u64 {
        0 => Err(ToolError::InvalidParams(
            "certificate secret cannot be empty".to_owned(),
        )),
        1..=MAX_CERT_BYTES => Ok(decoded),
        _ => Err(ToolError::LimitExceeded),
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
    let tmp_target = target.with_extension(format!(
        "{}.tmp",
        target.extension().and_then(|s| s.to_str()).unwrap_or("tmp")
    ));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let res = (|| -> Result<(), std::io::Error> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp_target)?;
            file.write_all(bytes)?;
            file.flush()?;
            Ok(())
        })();

        if let Err(e) = res {
            let _ = fs::remove_file(&tmp_target);
            return Err(ToolError::EngineFailure(format!(
                "failed to stage private certificate: {e}"
            )));
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(e) = fs::write(&tmp_target, bytes) {
            let _ = fs::remove_file(&tmp_target);
            return Err(ToolError::EngineFailure(format!(
                "failed to write private certificate: {e}"
            )));
        }
    }

    if let Err(e) = fs::rename(&tmp_target, target) {
        let _ = fs::remove_file(&tmp_target);
        return Err(ToolError::EngineFailure(format!(
            "failed to stage private file atomically: {e}"
        )));
    }

    Ok(SecretFileGuard(Some(target.to_path_buf())))
}

#[cfg(any(test, feature = "test-probe"))]
thread_local! {
    static TEST_TSA_CLIENT: std::cell::RefCell<Option<Client>> = const { std::cell::RefCell::new(None) };
}

/// Sets a thread-local test TSA client for loopback testing.
#[cfg(any(test, feature = "test-probe"))]
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

/// Checks whether a parsed URL matches an entry in a comma-separated TSA allowlist.
pub fn matches_tsa_allowlist(url: &Url, allowlist: &str) -> bool {
    let allowed_entries = allowlist
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty());

    allowed_entries.into_iter().any(|item| {
        if let Ok(item_url) = Url::parse(item) {
            if item_url.scheme() != url.scheme() {
                return false;
            }
            let item_host = item_url.host_str().unwrap_or_default();
            let url_host = url.host_str().unwrap_or_default();
            if !item_host.eq_ignore_ascii_case(url_host) {
                return false;
            }
            if item_url.port_or_known_default() != url.port_or_known_default() {
                return false;
            }
            let item_path = item_url.path().trim_end_matches('/');
            if !item_path.is_empty() {
                let url_path = url.path().trim_end_matches('/');
                if !url_path.starts_with(item_path) {
                    return false;
                }
            }
            true
        } else {
            let (h, p) = if let Some((h, p_str)) = item.split_once(':') {
                (h, p_str.parse::<u16>().ok())
            } else {
                (item, None)
            };
            h.eq_ignore_ascii_case(url.host_str().unwrap_or_default())
                && p.is_none_or(|port| Some(port) == url.port_or_known_default())
        }
    })
}

/// Enforces TSA policy: destination must match allowlist, or cloud mode forbids unconfigured TSA.
pub fn check_tsa_policy(
    url: &Url,
    allowlist: Option<&str>,
    is_cloud: bool,
) -> Result<(), ToolError> {
    if let Some(list) = allowlist {
        if !matches_tsa_allowlist(url, list) {
            return Err(ToolError::EngineFailure(
                "TSA URL is not in the allowed TSA list".to_owned(),
            ));
        }
    } else if is_cloud {
        return Err(ToolError::EngineFailure(
            "TSA requests are disabled in cloud unless an allowlist is configured".to_owned(),
        ));
    }
    Ok(())
}

/// Builder for the default TSA client: the SSRF-safe defaults plus a hard
/// refusal to follow redirects, so a TSA cannot bounce a request elsewhere.
pub(crate) fn tsa_client_builder() -> ClientBuilder {
    Client::builder().max_redirects(0)
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

    let is_cloud = std::env::var("ASHIFT_CLOUD_MODE").as_deref() == Ok("1");

    let allowlist = std::env::var("ASHIFT_TSA_ALLOWLIST").ok();
    check_tsa_policy(&url, allowlist.as_deref(), is_cloud)?;

    #[cfg(any(test, feature = "test-probe"))]
    let seam_client = TEST_TSA_CLIENT.with(|c| c.borrow().clone());
    #[cfg(not(any(test, feature = "test-probe")))]
    let seam_client: Option<Client> = None;

    let default_client;
    let net_client = match client.or(seam_client.as_ref()) {
        Some(c) => c,
        None => {
            default_client = tsa_client_builder().build().map_err(|_| {
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

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        process::Command,
        sync::Arc,
    };

    use super::*;
    use crate::net::{HttpResponse, MockResolver, MockTransport};

    const CHILD_CASE_VAR: &str = "ASHIFT_TSA_TEST_CASE";

    fn mock_client(status: u16) -> Client {
        let resolver = Arc::new(MockResolver::new());
        resolver.insert(
            "tsa.example.com",
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
                443,
            )],
        );
        let transport = Arc::new(MockTransport::fixed(HttpResponse {
            status,
            headers: vec![(
                "location".to_owned(),
                "https://tsa.example.com/redirected".to_owned(),
            )],
            body: b"token".to_vec(),
        }));
        Client::builder()
            .resolver(resolver)
            .transport(transport)
            .build()
            .unwrap()
    }

    #[test]
    fn default_tsa_client_refuses_redirects() {
        let resolver = Arc::new(MockResolver::new());
        resolver.insert(
            "tsa.example.com",
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
                443,
            )],
        );
        let transport = Arc::new(MockTransport::fixed(HttpResponse {
            status: 302,
            headers: vec![(
                "location".to_owned(),
                "https://tsa.example.com/redirected".to_owned(),
            )],
            body: Vec::new(),
        }));
        let client = tsa_client_builder()
            .resolver(resolver)
            .transport(transport)
            .build()
            .unwrap();

        let err = client
            .post(
                Url::parse("https://tsa.example.com/tsa").unwrap(),
                b"data".to_vec(),
                "application/timestamp-query",
            )
            .unwrap_err();
        assert!(
            matches!(err, NetError::TooManyRedirects { max: 0 }),
            "default TSA client must refuse redirects, got {err:?}"
        );
    }

    /// Runs inside a re-executed test process whose environment the parent
    /// test controls; does nothing in a normal run.
    #[test]
    fn tsa_env_child() {
        let Ok(case) = std::env::var(CHILD_CASE_VAR) else {
            return;
        };
        let workspace = Workspace::new().unwrap();
        let client = mock_client(200);
        let result = broker_tsa_timestamp(
            &workspace,
            "https://tsa.example.com/tsa",
            b"query",
            Some(&client),
        );
        match case.as_str() {
            "denied" => {
                let err = result.expect_err("cloud mode without allowlist must be denied");
                assert!(
                    err.to_string().contains("disabled in cloud"),
                    "unexpected error: {err}"
                );
            }
            "allowed" => {
                let guard = result.expect("TSA request must be permitted");
                let path = guard.0.as_ref().expect("response is staged");
                assert_eq!(std::fs::read(path).unwrap(), b"token");
            }
            other => panic!("unknown case {other}"),
        }
    }

    fn run_env_case(case: &str, cloud_mode: Option<&str>, allowlist: Option<&str>) {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "tools::sign::tests::tsa_env_child",
            "--nocapture",
        ])
        .env(CHILD_CASE_VAR, case)
        .env_remove("ASHIFT_CLOUD_MODE")
        .env_remove("ASHIFT_TSA_ALLOWLIST");
        if let Some(value) = cloud_mode {
            cmd.env("ASHIFT_CLOUD_MODE", value);
        }
        if let Some(value) = allowlist {
            cmd.env("ASHIFT_TSA_ALLOWLIST", value);
        }
        let out = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "child case {case} failed:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn cloud_mode_env_is_detected_by_broker() {
        run_env_case("denied", Some("1"), None);
        run_env_case("allowed", Some("1"), Some("https://tsa.example.com"));
        run_env_case("allowed", Some("0"), None);
        run_env_case("allowed", None, None);
    }

    #[test]
    fn resolve_cert_bytes_rejects_filesystem_paths() {
        for path in [
            "/private/var/folders/secret.p12",
            "/etc/passwd",
            "/proc/cpuinfo",
            "~/.netrc",
            "./cert.p12",
            "../cert.p12",
            r".\cert.p12",
            r"..\cert.p12",
            r"C:\cert.p12",
            "C:/cert.p12",
        ] {
            let err = resolve_cert_bytes(path).expect_err(path);
            assert!(
                err.to_string().contains("not a filesystem path"),
                "{path}: {err}"
            );
        }
    }

    #[test]
    fn resolve_cert_bytes_decodes_base64_and_bounds_size() {
        assert_eq!(resolve_cert_bytes(" aGVsbG8= \n").unwrap(), b"hello");
        assert!(resolve_cert_bytes("***not base64***").is_err());
        assert!(resolve_cert_bytes("").is_err());
    }
}
