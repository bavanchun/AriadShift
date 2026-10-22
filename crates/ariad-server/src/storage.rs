//! Object storage: keys, presigned URLs, streaming transfers.
//!
//! S3-compatible stores (SeaweedFS, R2) sign URLs themselves. The local store
//! has no signer, so the API signs its own `/v1/blobs/{token}` URLs with
//! HMAC-SHA256 and serves them (see `api::blobs`).

use std::{
    collections::BTreeMap,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration,
};

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use hmac::{KeyInit, Mac};
use object_store::{
    ObjectStore, ObjectStoreExt,
    aws::{AmazonS3, AmazonS3Builder, AmazonS3ConfigKey},
    buffered::BufWriter,
    local::LocalFileSystem,
    path::Path,
    signer::{HeaderName, HeaderValue, Method, SignedUrlOptions, Signer},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;
use uuid::Uuid;

use crate::config::StorageConfig;

type HmacSha256 = hmac::Hmac<Sha256>;

/// Lifetime of an upload URL.
pub const UPLOAD_URL_TTL: Duration = Duration::from_secs(15 * 60);

/// Lifetime of a download URL (§12).
pub const DOWNLOAD_URL_TTL: Duration = Duration::from_secs(5 * 60);

/// Header S3 checks against the body's SHA-256.
pub const CHECKSUM_HEADER: &str = "x-amz-checksum-sha256";

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Store(#[from] object_store::Error),
    #[error("object I/O failed")]
    Io(#[from] std::io::Error),
    #[error("object is larger than {0} bytes")]
    TooLarge(u64),
    #[error("storage configuration is invalid: {0}")]
    Config(String),
    #[error("this storage cannot presign URLs")]
    NoSigner,
}

impl StorageError {
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Store(object_store::Error::NotFound { .. }))
    }
}

/// Bytes kept from the start of an object for format detection.
pub const SNIFF_BYTES: usize = 64 * 1024;

/// Result of [`Storage::digest`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Digested {
    pub size: u64,
    pub sha256: String,
    pub prefix: Vec<u8>,
}

/// Whether a key holds an input or an output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    In,
    Out,
}

/// A request the client sends to transfer an object.
#[derive(Clone, Debug, PartialEq)]
pub struct Presigned {
    pub method: &'static str,
    pub url: Url,
    /// Headers the client must send unchanged.
    pub headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

/// Claims inside a local blob token.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlobClaims {
    /// `GET` or `PUT`.
    #[serde(rename = "m")]
    pub method: String,
    /// Object key.
    #[serde(rename = "k")]
    pub key: String,
    /// Expiry, Unix seconds.
    #[serde(rename = "e")]
    pub expires: i64,
    /// Exact size for uploads.
    #[serde(rename = "n", default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Expected SHA-256 (hex) for uploads.
    #[serde(rename = "h", default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Clone)]
enum Signing {
    S3(Arc<AmazonS3>),
    Local { base: Url, secret: Arc<[u8]> },
    None,
}

impl std::fmt::Debug for Signing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::S3(_) => "S3",
            Self::Local { .. } => "Local",
            Self::None => "None",
        })
    }
}

/// Object storage shared by the API and workers.
#[derive(Clone, Debug)]
pub struct Storage {
    store: Arc<dyn ObjectStore>,
    prefix: String,
    signing: Signing,
}

impl Storage {
    /// Builds storage. `public_url` and `secret` enable local blob URLs; workers
    /// pass `None` because they never presign.
    pub fn from_config(
        config: &StorageConfig,
        public_url: Option<&Url>,
        secret: Option<&[u8]>,
    ) -> Result<Self, StorageError> {
        match config {
            StorageConfig::S3 {
                bucket,
                prefix,
                public_endpoint,
                aws,
            } => {
                let builder = s3_builder(bucket, aws)?;
                let store = Arc::new(builder.clone().build()?);
                let signer = match public_endpoint {
                    Some(endpoint) => Arc::new(
                        builder
                            .with_config(AmazonS3ConfigKey::Endpoint, endpoint)
                            .build()?,
                    ),
                    None => Arc::clone(&store),
                };
                Ok(Self {
                    store,
                    prefix: prefix.clone(),
                    signing: Signing::S3(signer),
                })
            }
            StorageConfig::Local { root } => {
                std::fs::create_dir_all(root)?;
                let store = Arc::new(LocalFileSystem::new_with_prefix(root)?);
                let signing = match (public_url, secret) {
                    (Some(base), Some(secret)) => Signing::Local {
                        base: base.clone(),
                        secret: Arc::from(secret),
                    },
                    _ => Signing::None,
                };
                Ok(Self {
                    store,
                    prefix: String::new(),
                    signing,
                })
            }
        }
    }

    /// The key of an object: `t/{tenant}/{owner}/{in|out}/{sha256}`, where the
    /// owner is the upload (inputs) or the job (outputs).
    #[must_use]
    pub fn key(&self, tenant: Uuid, owner: Uuid, direction: Direction, sha256: &str) -> String {
        let dir = match direction {
            Direction::In => "in",
            Direction::Out => "out",
        };
        let key = format!("t/{tenant}/{owner}/{dir}/{sha256}");
        if self.prefix.is_empty() {
            key
        } else {
            format!("{}/{key}", self.prefix)
        }
    }

    /// A presigned `PUT` for exactly `size` bytes hashing to `sha256`. S3
    /// stores check the signed `x-amz-checksum-sha256`; local blob URLs check
    /// size and hash while streaming.
    pub async fn presign_put(
        &self,
        key: &str,
        size: u64,
        sha256_hex: &str,
    ) -> Result<Presigned, StorageError> {
        let expires_at = expiry(UPLOAD_URL_TTL);
        match &self.signing {
            Signing::S3(signer) => {
                let digest = hex::decode(sha256_hex)
                    .map_err(|_| StorageError::Config("sha256 is not hex".to_owned()))?;
                let checksum = STANDARD.encode(digest);
                // The signer cannot bind content-length; a matching SHA-256
                // fixes the exact bytes, and `complete` re-checks both.
                let options = SignedUrlOptions::default().with_signed_header(
                    HeaderName::from_static(CHECKSUM_HEADER),
                    HeaderValue::from_str(&checksum)
                        .map_err(|e| StorageError::Config(e.to_string()))?,
                );
                let url = signer
                    .signed_url_opts(Method::PUT, &Path::from(key), UPLOAD_URL_TTL, &options)
                    .await?;
                Ok(Presigned {
                    method: "PUT",
                    url,
                    headers: BTreeMap::from([(CHECKSUM_HEADER.to_owned(), checksum)]),
                    expires_at,
                })
            }
            Signing::Local { base, secret } => {
                let claims = BlobClaims {
                    method: "PUT".to_owned(),
                    key: key.to_owned(),
                    expires: expires_at.timestamp(),
                    size: Some(size),
                    sha256: Some(sha256_hex.to_owned()),
                };
                Ok(Presigned {
                    method: "PUT",
                    url: blob_url(base, secret, &claims)?,
                    headers: BTreeMap::new(),
                    expires_at,
                })
            }
            Signing::None => Err(StorageError::NoSigner),
        }
    }

    /// A presigned `GET` valid for [`DOWNLOAD_URL_TTL`].
    pub async fn presign_get(&self, key: &str) -> Result<(Url, DateTime<Utc>), StorageError> {
        let expires_at = expiry(DOWNLOAD_URL_TTL);
        match &self.signing {
            Signing::S3(signer) => {
                let url = signer
                    .signed_url(Method::GET, &Path::from(key), DOWNLOAD_URL_TTL)
                    .await?;
                Ok((url, expires_at))
            }
            Signing::Local { base, secret } => {
                let claims = BlobClaims {
                    method: "GET".to_owned(),
                    key: key.to_owned(),
                    expires: expires_at.timestamp(),
                    size: None,
                    sha256: None,
                };
                Ok((blob_url(base, secret, &claims)?, expires_at))
            }
            Signing::None => Err(StorageError::NoSigner),
        }
    }

    /// Verifies a local blob token and returns its claims.
    #[must_use]
    pub fn verify_blob_token(&self, token: &str, now: DateTime<Utc>) -> Option<BlobClaims> {
        let Signing::Local { secret, .. } = &self.signing else {
            return None;
        };
        let (payload, signature) = token.split_once('.')?;
        let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
        let mut mac = HmacSha256::new_from_slice(secret).ok()?;
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).ok()?;
        let claims: BlobClaims =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
        (claims.expires > now.timestamp()).then_some(claims)
    }

    /// Object size, or `None` when it does not exist.
    pub async fn size(&self, key: &str) -> Result<Option<u64>, StorageError> {
        match self.store.head(&Path::from(key)).await {
            Ok(meta) => Ok(Some(meta.size)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Streams an object, returning its size, SHA-256 and first
    /// [`SNIFF_BYTES`] bytes. Fails beyond `max_bytes`.
    pub async fn digest(&self, key: &str, max_bytes: u64) -> Result<Digested, StorageError> {
        let mut stream = self.store.get(&Path::from(key)).await?.into_stream();
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let mut prefix = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            size += chunk.len() as u64;
            if size > max_bytes {
                return Err(StorageError::TooLarge(max_bytes));
            }
            if prefix.len() < SNIFF_BYTES {
                let take = (SNIFF_BYTES - prefix.len()).min(chunk.len());
                prefix.extend_from_slice(&chunk[..take]);
            }
            hasher.update(&chunk);
        }
        Ok(Digested {
            size,
            sha256: hex::encode(hasher.finalize()),
            prefix,
        })
    }

    /// Downloads an object into `dest`, returning its size and SHA-256.
    pub async fn download(
        &self,
        key: &str,
        dest: &FsPath,
        max_bytes: u64,
    ) -> Result<(u64, String), StorageError> {
        let mut stream = self.store.get(&Path::from(key)).await?.into_stream();
        let mut file = tokio::fs::File::create(dest).await?;
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            total += chunk.len() as u64;
            if total > max_bytes {
                return Err(StorageError::TooLarge(max_bytes));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        file.sync_all().await?;
        Ok((total, hex::encode(hasher.finalize())))
    }

    /// Uploads a file, streaming it in multipart chunks when large.
    pub async fn upload_file(&self, key: &str, src: &FsPath) -> Result<(), StorageError> {
        let mut file = tokio::fs::File::open(src).await?;
        let mut writer = BufWriter::new(Arc::clone(&self.store), Path::from(key));
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            if let Err(err) = writer.write_all(&buf[..n]).await {
                let _ = writer.abort().await;
                return Err(err.into());
            }
        }
        writer.shutdown().await?;
        Ok(())
    }

    /// Writes a byte stream to `key` from a local blob `PUT`, enforcing the
    /// exact size and hash. On mismatch nothing remains under `key`.
    pub async fn put_stream<S, E>(
        &self,
        key: &str,
        mut body: S,
        size: u64,
        sha256_hex: &str,
    ) -> Result<bool, StorageError>
    where
        S: futures_util::Stream<Item = Result<bytes::Bytes, E>> + Unpin,
        E: std::fmt::Display,
    {
        let mut writer = BufWriter::new(Arc::clone(&self.store), Path::from(key));
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        while let Some(chunk) = body.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(err) => {
                    let _ = writer.abort().await;
                    return Err(StorageError::Io(std::io::Error::other(err.to_string())));
                }
            };
            total += chunk.len() as u64;
            if total > size {
                let _ = writer.abort().await;
                return Ok(false);
            }
            hasher.update(&chunk);
            if let Err(err) = writer.write_all(&chunk).await {
                let _ = writer.abort().await;
                return Err(err.into());
            }
        }
        if total != size || hex::encode(hasher.finalize()) != sha256_hex {
            let _ = writer.abort().await;
            return Ok(false);
        }
        writer.shutdown().await?;
        Ok(true)
    }

    /// Streams an object for a local blob `GET`.
    pub async fn get_stream(
        &self,
        key: &str,
    ) -> Result<
        (
            u64,
            futures_util::stream::BoxStream<'static, object_store::Result<bytes::Bytes>>,
        ),
        StorageError,
    > {
        let result = self.store.get(&Path::from(key)).await?;
        let size = result.meta.size;
        Ok((size, result.into_stream()))
    }

    /// Deletes an object; a missing object is not an error.
    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        match self.store.delete(&Path::from(key)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Cheap reachability probe for readiness checks.
    pub async fn probe(&self) -> Result<(), StorageError> {
        let key = if self.prefix.is_empty() {
            "readiness-probe".to_owned()
        } else {
            format!("{}/readiness-probe", self.prefix)
        };
        self.size(&key).await.map(|_| ())
    }
}

fn s3_builder(bucket: &str, aws: &[(String, String)]) -> Result<AmazonS3Builder, StorageError> {
    let mut builder = AmazonS3Builder::new().with_bucket_name(bucket);
    for (name, value) in aws {
        let key = name.to_ascii_lowercase();
        if key == "aws_bucket" || key == "aws_bucket_name" {
            continue;
        }
        if let Ok(config_key) = key.parse::<AmazonS3ConfigKey>() {
            builder = builder.with_config(config_key, value);
        }
    }
    Ok(builder)
}

fn expiry(ttl: Duration) -> DateTime<Utc> {
    Utc::now() + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::minutes(5))
}

fn blob_url(base: &Url, secret: &[u8], claims: &BlobClaims) -> Result<Url, StorageError> {
    let payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(claims).map_err(|e| StorageError::Config(e.to_string()))?);
    let mut mac =
        HmacSha256::new_from_slice(secret).map_err(|e| StorageError::Config(e.to_string()))?;
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let mut url = base.clone();
    let path = format!(
        "{}/v1/blobs/{payload}.{signature}",
        base.path().trim_end_matches('/')
    );
    url.set_path(&path);
    url.set_query(None);
    Ok(url)
}

/// Directory of a per-job workspace for the worker.
#[must_use]
pub fn job_dir(work_dir: &FsPath, job: Uuid) -> PathBuf {
    work_dir.join(format!("ariad-job-{}", job.simple()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(dir: &FsPath) -> Storage {
        Storage::from_config(
            &StorageConfig::Local {
                root: dir.to_path_buf(),
            },
            Some(&Url::parse("http://localhost:8080/").expect("url")),
            Some(b"0123456789abcdef0123456789abcdef"),
        )
        .expect("storage")
    }

    #[test]
    fn keys_follow_the_tenant_layout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = local(dir.path());
        let tenant = Uuid::nil();
        let owner = Uuid::from_u128(7);
        assert_eq!(
            storage.key(tenant, owner, Direction::Out, "ab"),
            format!("t/{tenant}/{owner}/out/ab")
        );
    }

    #[tokio::test]
    async fn blob_tokens_verify_and_expire() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = local(dir.path());
        let presigned = storage
            .presign_put("t/x/y/in/z", 3, &"0".repeat(64))
            .await
            .expect("presign");
        assert!(presigned.url.path().starts_with("/v1/blobs/"));
        let token = presigned
            .url
            .path()
            .trim_start_matches("/v1/blobs/")
            .to_owned();
        let claims = storage
            .verify_blob_token(&token, Utc::now())
            .expect("valid");
        assert_eq!(claims.method, "PUT");
        assert_eq!(claims.key, "t/x/y/in/z");
        assert_eq!(claims.size, Some(3));
        let later = Utc::now() + chrono::Duration::hours(1);
        assert!(storage.verify_blob_token(&token, later).is_none());

        let mut tampered = token.clone();
        tampered.insert(0, 'A');
        assert!(storage.verify_blob_token(&tampered, Utc::now()).is_none());
    }

    #[tokio::test]
    async fn put_stream_enforces_size_and_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = local(dir.path());
        let body = b"hello".to_vec();
        let sha = hex::encode(Sha256::digest(&body));
        let chunks = || {
            futures_util::stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from(
                body.clone(),
            ))])
        };
        assert!(
            !storage
                .put_stream("a/b", chunks(), 4, &sha)
                .await
                .expect("put")
        );
        assert_eq!(storage.size("a/b").await.expect("size"), None);
        assert!(
            !storage
                .put_stream("a/b", chunks(), 5, &"0".repeat(64))
                .await
                .expect("put")
        );
        assert!(
            storage
                .put_stream("a/b", chunks(), 5, &sha)
                .await
                .expect("put")
        );
        let digested = storage.digest("a/b", 5).await.expect("digest");
        assert_eq!((digested.size, digested.sha256), (5, sha));
        assert_eq!(digested.prefix, body);
        assert!(matches!(
            storage.digest("a/b", 4).await,
            Err(StorageError::TooLarge(4))
        ));
        storage.delete("a/b").await.expect("delete");
        storage.delete("a/b").await.expect("delete twice");
    }
}
