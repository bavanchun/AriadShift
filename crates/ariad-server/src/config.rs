//! Configuration from environment variables.
//!
//! Every setting is read from an [`Env`] map, so tests build configurations
//! without touching the process environment. `docs/server.md` is the
//! reference for each variable.

use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use thiserror::Error;
use url::Url;

use crate::{packs::Pack, sandbox::SandboxMode};

/// Minimum length of `ARIAD_SECRET`, in bytes.
pub const MIN_SECRET_BYTES: usize = 32;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{name} is invalid: {reason}")]
    Invalid { name: &'static str, reason: String },
}

fn invalid(name: &'static str, reason: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        name,
        reason: reason.into(),
    }
}

/// A snapshot of environment variables.
#[derive(Clone, Default)]
pub struct Env(BTreeMap<String, String>);

impl Env {
    /// Captures the variables of the current process that are valid UTF-8.
    #[must_use]
    pub fn from_process() -> Self {
        Self(std::env::vars().collect())
    }

    /// Builds an environment from explicit pairs.
    pub fn from_pairs<K, V>(pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        Self(
            pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }

    /// Returns a non-empty value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
    }

    fn required(&self, name: &'static str) -> Result<&str, ConfigError> {
        self.get(name).ok_or(ConfigError::Missing(name))
    }

    fn parse_or<T: FromStr>(&self, name: &'static str, default: T) -> Result<T, ConfigError>
    where
        T::Err: std::fmt::Display,
    {
        match self.get(name) {
            Some(raw) => raw
                .parse()
                .map_err(|e: T::Err| invalid(name, e.to_string())),
            None => Ok(default),
        }
    }

    fn positive_or<T>(&self, name: &'static str, default: T) -> Result<T, ConfigError>
    where
        T: FromStr + Default + PartialEq,
        T::Err: std::fmt::Display,
    {
        let value = self.parse_or(name, default)?;
        if value == T::default() {
            return Err(invalid(name, "must be greater than zero"));
        }
        Ok(value)
    }

    fn bool_or(&self, name: &'static str, default: bool) -> Result<bool, ConfigError> {
        match self.get(name).map(str::to_ascii_lowercase).as_deref() {
            None => Ok(default),
            Some("1" | "true" | "yes" | "on") => Ok(true),
            Some("0" | "false" | "no" | "off") => Ok(false),
            Some(other) => Err(invalid(
                name,
                format!("expected true or false, got '{other}'"),
            )),
        }
    }

    fn list(&self, name: &str) -> Vec<String> {
        self.get(name)
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn secs_or(&self, name: &'static str, default: u64) -> Result<Duration, ConfigError> {
        let secs: u64 = self.parse_or(name, default)?;
        if secs == 0 {
            return Err(invalid(name, "must be greater than zero"));
        }
        Ok(Duration::from_secs(secs))
    }
}

/// Where objects live.
#[derive(Clone, Eq, PartialEq)]
pub enum StorageConfig {
    /// An S3-compatible bucket (SeaweedFS, R2, AWS). Credentials and the
    /// endpoint come from the standard `AWS_*` variables.
    S3 {
        bucket: String,
        prefix: String,
        /// Endpoint used for presigned URLs when clients reach the store at a
        /// different address than the server does.
        public_endpoint: Option<String>,
        /// Raw `AWS_*` settings, passed to the S3 client builder.
        aws: Vec<(String, String)>,
    },
    /// A local directory. The API signs and serves object URLs itself.
    Local { root: PathBuf },
}

impl StorageConfig {
    /// Reads `ARIAD_STORAGE_URL` and, for S3, the `AWS_*` settings.
    pub fn from_env(env: &Env) -> Result<Self, ConfigError> {
        const NAME: &str = "ARIAD_STORAGE_URL";
        let raw = env.required(NAME)?;
        let url = Url::parse(raw).map_err(|e| invalid(NAME, e.to_string()))?;
        match url.scheme() {
            "s3" => {
                let bucket = url
                    .host_str()
                    .filter(|b| !b.is_empty())
                    .ok_or_else(|| invalid(NAME, "s3 URLs need a bucket, as in s3://bucket"))?
                    .to_owned();
                let prefix = url.path().trim_matches('/').to_owned();
                let aws = env
                    .0
                    .iter()
                    .filter(|(k, v)| k.starts_with("AWS_") && !v.trim().is_empty())
                    .map(|(k, v)| (k.clone(), v.trim().to_owned()))
                    .collect();
                Ok(Self::S3 {
                    bucket,
                    prefix,
                    public_endpoint: env.get("ARIAD_S3_PUBLIC_ENDPOINT").map(str::to_owned),
                    aws,
                })
            }
            "file" => {
                let root = url
                    .to_file_path()
                    .map_err(|()| invalid(NAME, "file URLs need an absolute path"))?;
                Ok(Self::Local { root })
            }
            other => Err(invalid(
                NAME,
                format!("unsupported scheme '{other}'; use s3:// or file://"),
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogFormat {
    Text,
    Json,
}

/// Settings shared by every subcommand.
#[derive(Clone)]
pub struct CommonConfig {
    pub database_url: String,
    pub database_max_connections: u32,
}

impl CommonConfig {
    pub fn from_env(env: &Env) -> Result<Self, ConfigError> {
        Ok(Self {
            database_url: env.required("DATABASE_URL")?.to_owned(),
            database_max_connections: env.parse_or("ARIAD_DATABASE_MAX_CONNECTIONS", 10)?,
        })
    }
}

/// Logging and OpenTelemetry settings.
#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    pub log_format: LogFormat,
    pub log_filter: String,
    pub otlp_endpoint: Option<String>,
    pub service_name: Option<String>,
}

impl TelemetryConfig {
    pub fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let log_format = match env.get("ARIAD_LOG_FORMAT") {
            None | Some("text") => LogFormat::Text,
            Some("json") => LogFormat::Json,
            Some(other) => {
                return Err(invalid(
                    "ARIAD_LOG_FORMAT",
                    format!("expected text or json, got '{other}'"),
                ));
            }
        };
        Ok(Self {
            log_format,
            log_filter: env.get("RUST_LOG").unwrap_or("info").to_owned(),
            otlp_endpoint: env.get("OTEL_EXPORTER_OTLP_ENDPOINT").map(str::to_owned),
            service_name: env.get("OTEL_SERVICE_NAME").map(str::to_owned),
        })
    }
}

/// How the API verifies Better Auth credentials.
#[derive(Clone, Debug)]
pub struct AuthConfig {
    /// JWT verification, when a Better Auth base URL is configured.
    pub jwt: Option<JwtConfig>,
    /// Whether `x-api-key` credentials are checked against `apikey`.
    pub api_keys: bool,
    /// Better Auth API key configurations whose keys belong to users.
    pub api_key_config_ids: Vec<String>,
    /// Whether callers without credentials may use the small-file tier.
    pub anonymous: bool,
    /// Cloudflare Turnstile verification for anonymous uploads.
    pub turnstile: Option<TurnstileConfig>,
}

#[derive(Clone, Debug)]
pub struct JwtConfig {
    pub jwks_url: Url,
    pub issuer: String,
    pub audience: String,
}

#[derive(Clone)]
pub struct TurnstileConfig {
    pub secret: String,
    pub verify_url: Url,
}

const TURNSTILE_VERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

impl AuthConfig {
    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let jwt = match env.get("ARIAD_AUTH_URL") {
            Some(raw) => {
                let base = Url::parse(raw).map_err(|e| invalid("ARIAD_AUTH_URL", e.to_string()))?;
                let origin = base.origin().ascii_serialization();
                if origin == "null" {
                    return Err(invalid("ARIAD_AUTH_URL", "needs an http or https origin"));
                }
                let jwks_url = match env.get("ARIAD_AUTH_JWKS_URL") {
                    Some(raw) => Url::parse(raw)
                        .map_err(|e| invalid("ARIAD_AUTH_JWKS_URL", e.to_string()))?,
                    None => Url::parse(&format!("{origin}/api/auth/jwks"))
                        .map_err(|e| invalid("ARIAD_AUTH_URL", e.to_string()))?,
                };
                Some(JwtConfig {
                    jwks_url,
                    issuer: env.get("ARIAD_AUTH_ISSUER").unwrap_or(&origin).to_owned(),
                    audience: env.get("ARIAD_AUTH_AUDIENCE").unwrap_or(&origin).to_owned(),
                })
            }
            None => None,
        };
        let mut api_key_config_ids = env.list("ARIAD_API_KEY_CONFIG_IDS");
        if api_key_config_ids.is_empty() {
            api_key_config_ids.push("default".to_owned());
        }
        let turnstile = match env.get("ARIAD_TURNSTILE_SECRET") {
            Some(secret) => Some(TurnstileConfig {
                secret: secret.to_owned(),
                verify_url: Url::parse(
                    env.get("ARIAD_TURNSTILE_VERIFY_URL")
                        .unwrap_or(TURNSTILE_VERIFY_URL),
                )
                .map_err(|e| invalid("ARIAD_TURNSTILE_VERIFY_URL", e.to_string()))?,
            }),
            None => None,
        };
        Ok(Self {
            jwt,
            api_keys: env.bool_or("ARIAD_API_KEYS", true)?,
            api_key_config_ids,
            anonymous: env.bool_or("ARIAD_ANONYMOUS", true)?,
            turnstile,
        })
    }
}

/// Per-process resource limits for local signed blob transfers.
#[derive(Clone, Copy, Debug)]
pub struct BlobTransferLimits {
    /// Transfers in flight on this process, uploads and downloads together.
    pub max_concurrent: usize,
    /// Transfers in flight for one workspace, so one tenant (or every
    /// anonymous caller together) cannot take the whole budget.
    pub max_per_workspace: usize,
    pub idle_timeout: Duration,
    pub transfer_timeout: Duration,
    /// Slowest average rate, in bytes per second, a transfer may keep once
    /// `rate_grace` has passed. A slot costs a client real bandwidth instead
    /// of one byte per idle period.
    pub min_bytes_per_sec: u64,
    pub rate_grace: Duration,
}

impl Default for BlobTransferLimits {
    fn default() -> Self {
        Self {
            max_concurrent: 64,
            max_per_workspace: 8,
            idle_timeout: Duration::from_secs(15),
            transfer_timeout: Duration::from_secs(15 * 60),
            min_bytes_per_sec: 16 * 1024,
            rate_grace: Duration::from_secs(30),
        }
    }
}

impl BlobTransferLimits {
    /// Reads the `ARIAD_BLOB_*` overrides; none of them can be zero.
    pub fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let defaults = Self::default();
        Ok(Self {
            max_concurrent: env.positive_or("ARIAD_BLOB_MAX_TRANSFERS", defaults.max_concurrent)?,
            max_per_workspace: env.positive_or(
                "ARIAD_BLOB_MAX_TRANSFERS_PER_WORKSPACE",
                defaults.max_per_workspace,
            )?,
            min_bytes_per_sec: env
                .positive_or("ARIAD_BLOB_MIN_RATE_BPS", defaults.min_bytes_per_sec)?,
            ..defaults
        })
    }
}

/// Settings for `ariad-server api`.
#[derive(Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    /// Public base URL of this API; local-store object URLs point here.
    pub public_url: Option<Url>,
    pub secret: Vec<u8>,
    pub cors_origins: Vec<String>,
    /// Packs with running workers; the planner only routes through them.
    pub packs: Vec<Pack>,
    pub auth: AuthConfig,
    /// Header carrying the client address when a trusted proxy sits in front.
    pub trusted_client_ip_header: Option<String>,
    pub request_timeout: Duration,
    pub sweep_interval: Duration,
    pub sweep_batch: i64,
    pub storage: StorageConfig,
    /// Budgets for the API-served blob URLs of `file://` storage.
    pub blob_transfers: BlobTransferLimits,
}

impl ApiConfig {
    pub fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let secret = env.required("ARIAD_SECRET")?.as_bytes().to_vec();
        if secret.len() < MIN_SECRET_BYTES {
            return Err(invalid(
                "ARIAD_SECRET",
                format!("must be at least {MIN_SECRET_BYTES} bytes"),
            ));
        }
        let storage = StorageConfig::from_env(env)?;
        let public_url = match env.get("ARIAD_PUBLIC_URL") {
            Some(raw) => {
                Some(Url::parse(raw).map_err(|e| invalid("ARIAD_PUBLIC_URL", e.to_string()))?)
            }
            None => None,
        };
        if matches!(storage, StorageConfig::Local { .. }) && public_url.is_none() {
            return Err(invalid(
                "ARIAD_PUBLIC_URL",
                "is required with file:// storage, because the API serves object URLs",
            ));
        }
        let sweep_batch: i64 = env.parse_or("ARIAD_SWEEP_BATCH", 200)?;
        if sweep_batch <= 0 {
            return Err(invalid("ARIAD_SWEEP_BATCH", "must be greater than zero"));
        }
        Ok(Self {
            bind: env.parse_or("ARIAD_BIND", SocketAddr::from(([0, 0, 0, 0], 8080)))?,
            public_url,
            secret,
            cors_origins: env.list("ARIAD_CORS_ORIGINS"),
            packs: parse_packs("ARIAD_PACKS", &env.list("ARIAD_PACKS"))?,
            auth: AuthConfig::from_env(env)?,
            trusted_client_ip_header: env
                .get("ARIAD_TRUSTED_CLIENT_IP_HEADER")
                .map(str::to_ascii_lowercase),
            request_timeout: env.secs_or("ARIAD_REQUEST_TIMEOUT_S", 30)?,
            sweep_interval: env.secs_or("ARIAD_SWEEP_INTERVAL_S", 60)?,
            sweep_batch,
            storage,
            blob_transfers: BlobTransferLimits::from_env(env)?,
        })
    }
}

/// Settings for `ariad-server worker`.
#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub packs: Vec<Pack>,
    pub concurrency: usize,
    pub max_attempts: u32,
    pub poll: Duration,
    pub engine_program: PathBuf,
    pub conversion_program: PathBuf,
    pub work_dir: PathBuf,
    pub sandbox: SandboxMode,
    pub storage: StorageConfig,
}

impl WorkerConfig {
    pub fn from_env(env: &Env, packs: &[String]) -> Result<Self, ConfigError> {
        let concurrency: usize = env.parse_or("ARIAD_WORKER_CONCURRENCY", 1)?;
        if concurrency == 0 {
            return Err(invalid("ARIAD_WORKER_CONCURRENCY", "must be at least 1"));
        }
        let max_attempts: u32 = env.parse_or("ARIAD_WORKER_MAX_ATTEMPTS", 3)?;
        if max_attempts == 0 {
            return Err(invalid("ARIAD_WORKER_MAX_ATTEMPTS", "must be at least 1"));
        }
        let engine_program = match env.get("ARIAD_ENGINE_PROGRAM") {
            Some(path) => PathBuf::from(path),
            None => default_engine_program().ok_or(ConfigError::Missing(
                "ARIAD_ENGINE_PROGRAM (no ashift binary found)",
            ))?,
        };
        Ok(Self {
            packs: parse_packs("--packs", packs)?,
            concurrency,
            max_attempts,
            poll: env.secs_or("ARIAD_WORKER_POLL_S", 5)?,
            engine_program,
            conversion_program: env
                .get("ARIAD_CONVERSION_PROGRAM")
                .map(PathBuf::from)
                .or_else(|| std::env::current_exe().ok())
                .ok_or(ConfigError::Missing("ARIAD_CONVERSION_PROGRAM"))?,
            work_dir: env.required("ARIAD_WORK_DIR").map(PathBuf::from)?,
            sandbox: match env.get("ARIAD_SANDBOX") {
                Some(raw) => raw
                    .parse()
                    .map_err(|e: String| invalid("ARIAD_SANDBOX", e))?,
                None => SandboxMode::platform_default(),
            },
            storage: StorageConfig::from_env(env)?,
        })
    }
}

fn parse_packs(name: &'static str, raw: &[String]) -> Result<Vec<Pack>, ConfigError> {
    let mut packs = Vec::new();
    for item in raw.iter().flat_map(|s| s.split(',')) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let pack: Pack = item.parse().map_err(|e: String| invalid(name, e))?;
        if !packs.contains(&pack) {
            packs.push(pack);
        }
    }
    if packs.is_empty() {
        packs.push(Pack::Core);
    }
    Ok(packs)
}

/// The `ashift` binary next to the running executable, else the first one on `PATH`.
#[must_use]
pub fn default_engine_program() -> Option<PathBuf> {
    let name = format!("ashift{}", std::env::consts::EXE_SUFFIX);
    if let Some(sibling) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|path| path.is_file())
    {
        return Some(sibling);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(&name))
            .find(|path| path.is_file())
    })
}

macro_rules! redacted_debug {
    ($($ty:ty),+ $(,)?) => {$ (
        impl std::fmt::Debug for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($ty)).finish_non_exhaustive()
            }
        }
    )+ };
}

redacted_debug!(Env, StorageConfig, CommonConfig, TurnstileConfig, ApiConfig);

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Vec<(String, String)> {
        [
            ("DATABASE_URL", "postgres://localhost/ariad"),
            ("ARIAD_SECRET", "0123456789abcdef0123456789abcdef"),
            ("ARIAD_STORAGE_URL", "s3://bucket/some/prefix/"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
    }

    fn with(mut pairs: Vec<(String, String)>, name: &str, value: &str) -> Vec<(String, String)> {
        pairs.retain(|(k, _)| k != name);
        pairs.push((name.to_owned(), value.to_owned()));
        pairs
    }

    #[test]
    fn api_config_reads_defaults() {
        let config = ApiConfig::from_env(&Env::from_pairs(base())).expect("config");
        assert_eq!(config.bind, SocketAddr::from(([0, 0, 0, 0], 8080)));
        assert_eq!(config.packs, vec![Pack::Core]);
        assert!(config.auth.jwt.is_none());
        assert!(config.auth.api_keys);
        assert!(config.auth.anonymous);
        assert_eq!(config.auth.api_key_config_ids, vec!["default".to_owned()]);
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        match config.storage {
            StorageConfig::S3 { bucket, prefix, .. } => {
                assert_eq!(bucket, "bucket");
                assert_eq!(prefix, "some/prefix");
            }
            StorageConfig::Local { .. } => panic!("expected s3"),
        }
    }

    #[test]
    fn workers_require_an_explicit_work_directory() {
        let pairs = with(base(), "ARIAD_ENGINE_PROGRAM", "ashift");
        assert!(matches!(
            WorkerConfig::from_env(&Env::from_pairs(pairs), &[]),
            Err(ConfigError::Missing("ARIAD_WORK_DIR"))
        ));
    }

    #[test]
    fn sweep_settings_require_positive_values() {
        let defaults = ApiConfig::from_env(&Env::from_pairs(base())).unwrap();
        assert_eq!(defaults.sweep_batch, 200);
        assert_eq!(defaults.sweep_interval, Duration::from_secs(60));
        for (name, value) in [
            ("ARIAD_SWEEP_BATCH", "0"),
            ("ARIAD_SWEEP_BATCH", "-1"),
            ("ARIAD_SWEEP_INTERVAL_S", "0"),
        ] {
            assert!(ApiConfig::from_env(&Env::from_pairs(with(base(), name, value))).is_err());
        }
        let config =
            ApiConfig::from_env(&Env::from_pairs(with(base(), "ARIAD_SWEEP_BATCH", "17"))).unwrap();
        assert_eq!(config.sweep_batch, 17);
    }

    #[test]
    fn blob_transfer_limits_read_positive_overrides() {
        let defaults = ApiConfig::from_env(&Env::from_pairs(base())).unwrap();
        assert_eq!(defaults.blob_transfers.max_concurrent, 64);
        assert_eq!(defaults.blob_transfers.max_per_workspace, 8);
        assert_eq!(defaults.blob_transfers.min_bytes_per_sec, 16 * 1024);
        let pairs = with(
            with(
                with(base(), "ARIAD_BLOB_MAX_TRANSFERS", "16"),
                "ARIAD_BLOB_MAX_TRANSFERS_PER_WORKSPACE",
                "2",
            ),
            "ARIAD_BLOB_MIN_RATE_BPS",
            "4096",
        );
        let config = ApiConfig::from_env(&Env::from_pairs(pairs)).unwrap();
        assert_eq!(config.blob_transfers.max_concurrent, 16);
        assert_eq!(config.blob_transfers.max_per_workspace, 2);
        assert_eq!(config.blob_transfers.min_bytes_per_sec, 4096);
        for name in [
            "ARIAD_BLOB_MAX_TRANSFERS",
            "ARIAD_BLOB_MAX_TRANSFERS_PER_WORKSPACE",
            "ARIAD_BLOB_MIN_RATE_BPS",
        ] {
            for value in ["0", "-1", "many"] {
                assert!(ApiConfig::from_env(&Env::from_pairs(with(base(), name, value))).is_err());
            }
        }
    }

    #[test]
    fn auth_url_derives_jwks_issuer_and_audience() {
        let pairs = with(
            base(),
            "ARIAD_AUTH_URL",
            "https://ariadshift.example/some/path",
        );
        let config = ApiConfig::from_env(&Env::from_pairs(pairs)).expect("config");
        let jwt = config.auth.jwt.expect("jwt");
        assert_eq!(
            jwt.jwks_url.as_str(),
            "https://ariadshift.example/api/auth/jwks"
        );
        assert_eq!(jwt.issuer, "https://ariadshift.example");
        assert_eq!(jwt.audience, "https://ariadshift.example");
    }

    #[test]
    fn short_secret_is_rejected() {
        let pairs = with(base(), "ARIAD_SECRET", "short");
        let err = ApiConfig::from_env(&Env::from_pairs(pairs)).expect_err("short secret");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                name: "ARIAD_SECRET",
                ..
            }
        ));
    }

    #[test]
    fn local_storage_requires_a_public_url() {
        let root = std::env::temp_dir().join("ariad-objects");
        let url = Url::from_directory_path(&root)
            .expect("dir url")
            .to_string();
        let pairs = with(base(), "ARIAD_STORAGE_URL", &url);
        let err = ApiConfig::from_env(&Env::from_pairs(pairs.clone())).expect_err("no public url");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                name: "ARIAD_PUBLIC_URL",
                ..
            }
        ));
        let pairs = with(pairs, "ARIAD_PUBLIC_URL", "http://localhost:8080");
        let config = ApiConfig::from_env(&Env::from_pairs(pairs)).expect("config");
        assert!(matches!(config.storage, StorageConfig::Local { .. }));
    }

    #[test]
    fn packs_are_parsed_and_deduplicated() {
        let packs =
            parse_packs("--packs", &["core,office".to_owned(), "core".to_owned()]).expect("packs");
        assert_eq!(packs, vec![Pack::Core, Pack::Office]);
        assert!(parse_packs("--packs", &["ocr".to_owned()]).is_err());
    }

    #[test]
    fn unknown_storage_scheme_is_rejected() {
        let pairs = with(base(), "ARIAD_STORAGE_URL", "gs://bucket");
        assert!(ApiConfig::from_env(&Env::from_pairs(pairs)).is_err());
    }

    #[test]
    fn boolean_values_are_strict() {
        let pairs = with(base(), "ARIAD_ANONYMOUS", "maybe");
        assert!(ApiConfig::from_env(&Env::from_pairs(pairs)).is_err());
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;

    #[test]
    fn configuration_debug_never_includes_secrets() {
        let sentinel = "credential-that-must-never-appear-in-debug";
        let env = Env::from_pairs([
            ("DATABASE_URL", sentinel),
            ("ARIAD_SECRET", sentinel),
            ("ARIAD_STORAGE_URL", "s3://bucket"),
            ("AWS_SECRET_ACCESS_KEY", sentinel),
            ("ARIAD_TURNSTILE_SECRET", sentinel),
            ("UNRELATED_SECRET", sentinel),
            ("ARIAD_ENGINE_PROGRAM", "ashift"),
            ("ARIAD_WORK_DIR", "/tmp/ariad-debug-test"),
        ]);
        let api = ApiConfig::from_env(&env).unwrap();
        for debug in [
            format!("{env:?}"),
            format!("{:?}", CommonConfig::from_env(&env).unwrap()),
            format!("{api:?}"),
            format!("{:?}", api.auth),
            format!("{:?}", api.storage),
            format!("{:?}", WorkerConfig::from_env(&env, &[]).unwrap()),
        ] {
            assert!(!debug.contains(sentinel), "{debug}");
        }
    }
}
