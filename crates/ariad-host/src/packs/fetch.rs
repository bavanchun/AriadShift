//! Fetching manifests and components over HTTPS, loopback HTTP or from the file system.

use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    thread,
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};

use super::{PackError, manifest::relative_path};

const CHUNK_BYTES: usize = 256 * 1024;

/// Network behaviour of pack downloads.
#[derive(Clone, Debug)]
pub struct FetchPolicy {
    /// Attempts per file, the first included.
    pub attempts: u32,
    /// Delay before the second attempt; doubled for each later one.
    pub backoff: Duration,
    /// TCP, proxy and TLS connection setup budget.
    pub connect_timeout: Duration,
    /// Longest wait for any single read; a stalled transfer fails after this.
    pub read_timeout: Duration,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            backoff: Duration::from_secs(1),
            connect_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(60),
        }
    }
}

/// Where a manifest, signature or component lives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Location {
    Url(String),
    Path(PathBuf),
}

impl Location {
    /// `https://` and loopback `http://` URLs are remote; anything without a scheme is a path.
    pub(crate) fn parse(text: &str) -> Result<Self, PackError> {
        if text.starts_with("https://") || text.starts_with("http://") {
            check_url(text)?;
            Ok(Self::Url(text.to_owned()))
        } else if text.contains("://") {
            Err(PackError::Usage(format!(
                "unsupported URL scheme in '{text}'; use https:// or a local path"
            )))
        } else {
            Ok(Self::Path(PathBuf::from(text)))
        }
    }

    /// Resolves a component URL: absolute URLs stand alone, anything else is a path relative to
    /// this location's directory.
    pub(crate) fn resolve(&self, reference: &str) -> Result<Self, PackError> {
        if reference.contains("://") {
            return Self::parse(reference);
        }
        let parts = relative_path(reference).map_err(|reason| {
            PackError::InvalidManifest(format!("component url '{reference}': {reason}"))
        })?;
        Ok(match self {
            Self::Url(base) => {
                let directory = &base[..=base.rfind('/').unwrap_or(base.len() - 1)];
                Self::Url(format!("{directory}{}", parts.join("/")))
            }
            Self::Path(base) => {
                let mut path = base.parent().map(Path::to_path_buf).unwrap_or_default();
                for part in parts {
                    path.push(part);
                }
                Self::Path(path)
            }
        })
    }

    /// The same location with `suffix` appended (`pack.toml` → `pack.toml.minisig`).
    pub(crate) fn with_suffix(&self, suffix: &str) -> Self {
        match self {
            Self::Url(url) => Self::Url(format!("{url}{suffix}")),
            Self::Path(path) => {
                let mut text = path.clone().into_os_string();
                text.push(suffix);
                Self::Path(PathBuf::from(text))
            }
        }
    }

    pub(crate) fn display(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::Path(path) => path.display().to_string(),
        }
    }
}

/// Remote URLs must use HTTPS; plain HTTP is accepted for loopback hosts only.
fn check_url(url: &str) -> Result<(), PackError> {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(PackError::Usage(format!(
            "invalid URL '{url}': a host is required and credentials are not allowed"
        )));
    }
    if scheme == "http" && !is_loopback(authority) {
        return Err(PackError::Usage(format!(
            "refusing plain HTTP for '{url}'; use https:// (HTTP is allowed for loopback hosts only)"
        )));
    }
    Ok(())
}

fn is_loopback(authority: &str) -> bool {
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Trust anchors from the operating system store (`SSL_CERT_FILE` and `SSL_CERT_DIR` are
/// honoured on Linux), loaded once per process.
fn native_roots() -> &'static Arc<Vec<Certificate<'static>>> {
    static ROOTS: OnceLock<Arc<Vec<Certificate<'static>>>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let loaded = rustls_native_certs::load_native_certs();
        Arc::new(
            loaded
                .certs
                .iter()
                .map(|der| Certificate::from_der(der.as_ref()).to_owned())
                .collect(),
        )
    })
}

/// Downloads with bounded retries and streams files while hashing them.
pub(crate) struct Fetcher {
    https: ureq::Agent,
    loopback: ureq::Agent,
    policy: FetchPolicy,
}

enum Attempt {
    Retry(String),
    Fail(PackError),
}

impl Fetcher {
    pub(crate) fn new(policy: FetchPolicy) -> Self {
        let agent = |https_only: bool| {
            ureq::Agent::config_builder()
                .https_only(https_only)
                .http_status_as_error(false)
                .user_agent(format!("ashift/{}", env!("CARGO_PKG_VERSION")))
                .timeout_connect(Some(policy.connect_timeout))
                .timeout_per_read(Some(policy.read_timeout))
                .tls_config(
                    TlsConfig::builder()
                        .provider(TlsProvider::Rustls)
                        .root_certs(RootCerts::Specific(Arc::clone(native_roots())))
                        .build(),
                )
                .build()
                .new_agent()
        };
        Self {
            https: agent(true),
            loopback: agent(false),
            policy,
        }
    }

    /// Reads a small file (manifest or signature) of at most `max_bytes`.
    pub(crate) fn read_small(
        &self,
        location: &Location,
        max_bytes: u64,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, PackError> {
        match location {
            Location::Path(path) => {
                let file = File::open(path)
                    .map_err(|error| PackError::io("could not open", path, error))?;
                let mut bytes = Vec::new();
                file.take(max_bytes + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| PackError::io("could not read", path, error))?;
                if bytes.len() as u64 > max_bytes {
                    return Err(PackError::InvalidManifest(format!(
                        "{} is larger than {max_bytes} bytes",
                        path.display()
                    )));
                }
                Ok(bytes)
            }
            Location::Url(url) => self.with_retries(url, cancel, |reader| {
                let mut bytes = Vec::new();
                reader
                    .take(max_bytes + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| Attempt::Retry(error.to_string()))?;
                if bytes.len() as u64 > max_bytes {
                    return Err(Attempt::Fail(PackError::Download {
                        url: url.clone(),
                        detail: format!("response is larger than {max_bytes} bytes"),
                    }));
                }
                Ok(bytes)
            }),
        }
    }

    /// Streams `location` into `out` while hashing it. Fails on a size or SHA-256 mismatch.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fetch_verified(
        &self,
        location: &Location,
        out: &mut File,
        out_path: &Path,
        component: &str,
        size: u64,
        sha256: &str,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), PackError> {
        let actual = match location {
            Location::Path(path) => {
                let mut file = File::open(path)
                    .map_err(|error| PackError::io("could not open", path, error))?;
                copy_hashed(&mut file, out, out_path, component, size, cancel, progress).map_err(
                    |attempt| match attempt {
                        Attempt::Retry(detail) => PackError::Io {
                            context: format!("could not read {}", path.display()),
                            source: io::Error::other(detail),
                        },
                        Attempt::Fail(error) => error,
                    },
                )?
            }
            Location::Url(url) => self.with_retries(url, cancel, |reader| {
                out.set_len(0)
                    .and_then(|()| out.seek(SeekFrom::Start(0)).map(drop))
                    .map_err(|error| {
                        Attempt::Fail(PackError::io("could not reset", out_path, error))
                    })?;
                copy_hashed(reader, out, out_path, component, size, cancel, progress)
            })?,
        };
        if actual != sha256 {
            return Err(PackError::HashMismatch {
                component: component.to_owned(),
                expected: sha256.to_owned(),
                actual,
            });
        }
        Ok(())
    }

    fn with_retries<T>(
        &self,
        url: &str,
        cancel: &CancellationToken,
        mut body: impl FnMut(&mut dyn Read) -> Result<T, Attempt>,
    ) -> Result<T, PackError> {
        let agent = if url.starts_with("https://") {
            &self.https
        } else {
            &self.loopback
        };
        let attempts = self.policy.attempts.max(1);
        let mut delay = self.policy.backoff;
        let mut last = String::new();
        for attempt in 1..=attempts {
            if cancel.is_cancelled() {
                return Err(PackError::Interrupted);
            }
            if attempt > 1 {
                sleep_cancellable(delay, cancel)?;
                delay = delay.saturating_mul(2);
            }
            let outcome = match agent.get(url).call() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if status == 200 {
                        let mut reader = response.into_body().into_reader();
                        body(&mut reader)
                    } else if status == 408 || status == 429 || status >= 500 {
                        Err(Attempt::Retry(format!("HTTP {status}")))
                    } else {
                        Err(Attempt::Fail(PackError::Download {
                            url: url.to_owned(),
                            detail: format!("HTTP {status}"),
                        }))
                    }
                }
                Err(error) => Err(classify(url, &error)),
            };
            match outcome {
                Ok(value) => return Ok(value),
                Err(Attempt::Fail(error)) => return Err(error),
                Err(Attempt::Retry(detail)) => last = detail,
            }
        }
        Err(PackError::Download {
            url: url.to_owned(),
            detail: format!("{last} (after {attempts} attempts)"),
        })
    }
}

fn classify(url: &str, error: &ureq::Error) -> Attempt {
    match error {
        ureq::Error::Io(_)
        | ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Protocol(_) => Attempt::Retry(error.to_string()),
        _ if url.starts_with("https://") && native_roots().is_empty() => {
            Attempt::Fail(PackError::Download {
                url: url.to_owned(),
                detail: format!(
                    "{error}; no trusted root certificates were found, set SSL_CERT_FILE to a CA bundle"
                ),
            })
        }
        _ => Attempt::Fail(PackError::Download {
            url: url.to_owned(),
            detail: error.to_string(),
        }),
    }
}

fn copy_hashed(
    reader: &mut dyn Read,
    out: &mut File,
    out_path: &Path,
    component: &str,
    size: u64,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(u64),
) -> Result<String, Attempt> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK_BYTES];
    let mut total = 0_u64;
    loop {
        if cancel.is_cancelled() {
            return Err(Attempt::Fail(PackError::Interrupted));
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(Attempt::Retry(error.to_string())),
        };
        total += read as u64;
        if total > size {
            return Err(Attempt::Fail(PackError::SizeMismatch {
                component: component.to_owned(),
                expected: size,
                actual: total,
            }));
        }
        hasher.update(&buffer[..read]);
        out.write_all(&buffer[..read])
            .map_err(|error| Attempt::Fail(PackError::io("could not write", out_path, error)))?;
        progress(total);
    }
    if total != size {
        return Err(Attempt::Retry(format!(
            "component '{component}' ended after {total} of {size} bytes"
        )));
    }
    out.flush()
        .map_err(|error| Attempt::Fail(PackError::io("could not write", out_path, error)))?;
    Ok(hex::encode(hasher.finalize()))
}

fn sleep_cancellable(delay: Duration, cancel: &CancellationToken) -> Result<(), PackError> {
    let step = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < delay {
        if cancel.is_cancelled() {
            return Err(PackError::Interrupted);
        }
        let next = step.min(delay - waited);
        thread::sleep(next);
        waited += next;
    }
    Ok(())
}

/// Hashes a local file.
pub(crate) fn sha256_file(
    path: &Path,
    cancel: &CancellationToken,
) -> Result<(String, u64), PackError> {
    let mut file =
        File::open(path).map_err(|error| PackError::io("could not open", path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK_BYTES];
    let mut total = 0_u64;
    loop {
        if cancel.is_cancelled() {
            return Err(PackError::Interrupted);
        }
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(PackError::io("could not read", path, error)),
        };
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((hex::encode(hasher.finalize()), total))
}

/// Removes a file, ignoring a missing one.
pub(crate) fn remove_file_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_require_https_except_on_loopback() {
        assert!(Location::parse("https://example.com/pack.toml").is_ok());
        assert!(Location::parse("http://127.0.0.1:8080/pack.toml").is_ok());
        assert!(Location::parse("http://localhost/pack.toml").is_ok());
        assert!(Location::parse("http://[::1]:9/pack.toml").is_ok());
        assert!(Location::parse("http://example.com/pack.toml").is_err());
        assert!(Location::parse("http://127.0.0.1.example.com/x").is_err());
        assert!(Location::parse("https://user:pw@example.com/x").is_err());
        assert!(Location::parse("ftp://example.com/x").is_err());
        assert_eq!(
            Location::parse("dir/pack.toml").unwrap(),
            Location::Path(PathBuf::from("dir/pack.toml"))
        );
    }

    #[test]
    fn relative_component_urls_resolve_beside_the_manifest() {
        let base = Location::parse("https://h/releases/download/pack-x-v1.0.0/pack.toml").unwrap();
        assert_eq!(
            base.resolve("x-runtime.tar.zst").unwrap(),
            Location::Url("https://h/releases/download/pack-x-v1.0.0/x-runtime.tar.zst".to_owned())
        );
        assert_eq!(
            base.resolve("https://cdn/x.zip").unwrap(),
            Location::Url("https://cdn/x.zip".to_owned())
        );
        assert!(base.resolve("../x.zip").is_err());
        assert!(base.resolve("http://evil.example/x.zip").is_err());
        let local = Location::Path(PathBuf::from("dir").join("pack.toml"));
        assert_eq!(
            local.resolve("sub/a.zip").unwrap(),
            Location::Path(PathBuf::from("dir").join("sub").join("a.zip"))
        );
        assert_eq!(
            local.with_suffix(".minisig"),
            Location::Path(PathBuf::from("dir").join("pack.toml.minisig"))
        );
    }
}
