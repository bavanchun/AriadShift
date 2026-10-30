//! Shared harness for the database-backed integration tests.
//!
//! Each test gets its own database on the server named by
//! `ARIAD_TEST_DATABASE_URL` (PostgreSQL with the pgmq extension available)
//! and skips with a message when the variable is unset, unless
//! `ARIAD_TEST_REQUIRE_DATABASE=1` makes a missing URL fatal. The API runs in
//! process on a random port with a local object store; workers use the
//! `ashift` binary as their engine program, as in production.
#![allow(dead_code)]

mod signing_key;
use signing_key::random_signing_key;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, OnceLock},
    time::Duration,
};

use ariad_server::{
    app,
    config::{ApiConfig, Env, WorkerConfig},
    db,
};
use axum::{Json, Router, routing::get, routing::post};
use jsonwebtoken::{Algorithm, EncodingKey, Header, jwk::Jwk};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

pub static SECRET: LazyLock<String> = LazyLock::new(|| hex::encode(random_signing_key()));

/// The admin connection string, or `None` to skip.
pub fn admin_url(test: &str) -> Option<String> {
    match std::env::var("ARIAD_TEST_DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            assert!(
                std::env::var("ARIAD_TEST_REQUIRE_DATABASE").as_deref() != Ok("1"),
                "{test}: ARIAD_TEST_REQUIRE_DATABASE=1 needs ARIAD_TEST_DATABASE_URL"
            );
            eprintln!("skipping {test}: ARIAD_TEST_DATABASE_URL is not set");
            None
        }
    }
}

/// A database created for one test.
pub struct TestDb {
    pub pool: PgPool,
    pub url: String,
    name: String,
    admin: String,
    runtime_logins: Vec<String>,
}

impl TestDb {
    /// Creates an empty database. Returns `None` (after printing why) when
    /// no test server is configured.
    pub async fn create_empty(test: &str) -> Option<Self> {
        let admin = admin_url(test)?;
        ariad_server::install_crypto_provider();
        let name = format!("ariad_test_{}", Uuid::new_v4().simple());
        let admin_pool = PgPool::connect(&admin)
            .await
            .expect("connect to the test server");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .execute(&admin_pool)
            .await
            .expect("create the test database");
        admin_pool.close().await;
        let mut url = Url::parse(&admin).expect("test URL");
        url.set_path(&format!("/{name}"));
        let pool = db::connect(url.as_str(), 8)
            .await
            .expect("connect to the test database");
        Some(Self {
            pool,
            url: url.to_string(),
            name,
            admin,
            runtime_logins: Vec::new(),
        })
    }

    /// Creates a database with every migration applied.
    pub async fn create(test: &str) -> Option<Self> {
        let db = Self::create_empty(test).await?;
        db::migrate(&db.pool).await.expect("migrate");
        Some(db)
    }

    /// Role names are cluster-wide even though each test owns a database.
    /// Serialize their first creation through the shared admin database.
    pub async fn apply_runtime_roles(&self) {
        let admin = PgPool::connect(&self.admin)
            .await
            .expect("role setup connection");
        let mut tx = admin.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(781124401)")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../../../infra/server-roles.sql"))
            .execute(&self.pool)
            .await
            .expect("apply runtime grants");
        tx.commit().await.unwrap();
        admin.close().await;
    }

    pub async fn drop_database(self) {
        self.pool.close().await;
        if let Ok(admin) = PgPool::connect(&self.admin).await {
            let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                self.name
            )))
            .execute(&admin)
            .await;
            for login in self.runtime_logins {
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP ROLE \"{login}\"")))
                    .execute(&admin)
                    .await
                    .expect("drop runtime login");
            }
            admin.close().await;
        }
    }
}

/// How a request authenticates.
#[derive(Clone, Debug)]
pub enum Auth {
    Anonymous,
    Bearer(String),
    ApiKey(String),
}

impl Auth {
    pub fn apply(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            Self::Anonymous => request,
            Self::Bearer(token) => request.bearer_auth(token),
            Self::ApiKey(key) => request.header("x-api-key", key),
        }
    }
}

/// Options for [`Harness::start`].
#[derive(Default)]
pub struct Options {
    /// Extra or overriding API environment variables.
    pub env: Vec<(String, String)>,
    /// Keep the seeded quotas and rate limits instead of lifting them.
    pub keep_quotas: bool,
    pub restricted_logins: bool,
}

/// API and optional worker on a fresh database.
pub struct Harness {
    pub db: TestDb,
    pub base: Url,
    pub http: reqwest::Client,
    pub objects: tempfile::TempDir,
    pub storage_env: Vec<(String, String)>,
    shutdown: CancellationToken,
    api: JoinHandle<()>,
    workers: Vec<(CancellationToken, JoinHandle<()>)>,
    work_dir: tempfile::TempDir,
    worker_url: Option<String>,
}

impl Harness {
    pub async fn start(test: &str, options: Options) -> Option<Self> {
        let mut db = TestDb::create(test).await?;
        if !options.keep_quotas {
            sqlx::query(
                "UPDATE plan_entitlements SET jobs_per_day = NULL, input_bytes_per_day = NULL, \
                 concurrent_jobs = NULL, requests_per_minute = NULL",
            )
            .execute(&db.pool)
            .await
            .expect("lift quotas");
        }
        let mut api_pool = db.pool.clone();
        let mut worker_url = None;
        if options.restricted_logins {
            create_apikey_table(&db.pool).await;
            db.apply_runtime_roles().await;
            for role in ["ariad_api", "ariad_worker"] {
                let login = format!("runtime_{}", Uuid::new_v4().simple());
                let password = Uuid::new_v4().simple().to_string();
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "CREATE ROLE \"{login}\" LOGIN PASSWORD '{password}' IN ROLE {role}"
                )))
                .execute(&db.pool)
                .await
                .expect("create restricted login");
                let mut url = Url::parse(&db.url).unwrap();
                url.set_username(&login).unwrap();
                url.set_password(Some(&password)).unwrap();
                let pool = db::connect(url.as_str(), 8)
                    .await
                    .expect("restricted connection");
                let elevated: bool = sqlx::query_scalar("SELECT rolsuper OR rolcreaterole OR rolbypassrls FROM pg_roles WHERE rolname = current_user")
                    .fetch_one(&pool).await.unwrap();
                assert!(!elevated);
                if role == "ariad_api" {
                    api_pool = pool;
                } else {
                    pool.close().await;
                    worker_url = Some(url.to_string());
                }
                db.runtime_logins.push(login);
            }
        }
        let objects = tempfile::tempdir().expect("object dir");
        let storage_url = Url::from_directory_path(objects.path()).expect("file URL");
        let mut storage_env = vec![("ARIAD_STORAGE_URL".to_owned(), storage_url.to_string())];
        let mut env = options.env;
        if let Some((_, url)) = env.iter().find(|(k, _)| k == "ARIAD_STORAGE_URL") {
            storage_env = vec![("ARIAD_STORAGE_URL".to_owned(), url.clone())];
            storage_env.extend(env.iter().filter(|(k, _)| k.starts_with("AWS_")).cloned());
            if let Some(public) = env.iter().find(|(k, _)| k == "ARIAD_S3_PUBLIC_ENDPOINT") {
                storage_env.push(public.clone());
            }
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base = Url::parse(&format!("http://{addr}/")).expect("base URL");
        let mut pairs = vec![
            ("DATABASE_URL".to_owned(), db.url.clone()),
            ("ARIAD_SECRET".to_owned(), SECRET.as_str().to_owned()),
            ("ARIAD_PUBLIC_URL".to_owned(), base.to_string()),
            ("ARIAD_SWEEP_INTERVAL_S".to_owned(), "3600".to_owned()),
        ];
        pairs.extend(storage_env.clone());
        pairs.append(&mut env);
        let config = ApiConfig::from_env(&Env::from_pairs(pairs)).expect("API config");
        let shutdown = CancellationToken::new();
        let pool = api_pool;
        let api_shutdown = shutdown.clone();
        let api = tokio::spawn(async move {
            app::serve_api(listener, config, pool, api_shutdown)
                .await
                .expect("serve API");
        });
        Some(Self {
            db,
            base,
            http: ariad_server::http_client(),
            objects,
            storage_env,
            shutdown,
            api,
            workers: Vec::new(),
            work_dir: tempfile::tempdir().expect("work dir"),
            worker_url,
        })
    }

    /// Starts a worker for the core pack.
    pub fn start_worker(&mut self) {
        self.start_worker_with(&[]);
    }

    pub fn start_worker_with(&mut self, extra: &[(&str, &str)]) {
        let mut pairs: Vec<(String, String)> = vec![
            ("DATABASE_URL".to_owned(), self.db.url.clone()),
            ("ARIAD_SANDBOX".to_owned(), "off".to_owned()),
            ("ARIAD_WORKER_POLL_S".to_owned(), "1".to_owned()),
            (
                "ARIAD_ENGINE_PROGRAM".to_owned(),
                ashift().display().to_string(),
            ),
            (
                "ARIAD_WORK_DIR".to_owned(),
                self.work_dir.path().display().to_string(),
            ),
        ];
        pairs.extend(self.storage_env.clone());
        pairs.extend(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        pairs.push((
            "ARIAD_CONVERSION_PROGRAM".to_owned(),
            env!("CARGO_BIN_EXE_ariad-server").to_owned(),
        ));
        let config = WorkerConfig::from_env(&Env::from_pairs(pairs), &["core".to_owned()])
            .expect("worker config");
        let token = self.shutdown.child_token();
        let pool = if let Some(url) = &self.worker_url {
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(8)
                .connect_lazy(url)
                .expect("restricted worker pool")
        } else if let Some((_, size)) = extra
            .iter()
            .find(|(name, _)| *name == "ARIAD_DATABASE_MAX_CONNECTIONS")
        {
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(size.parse().expect("worker pool size"))
                .connect_lazy(&self.db.url)
                .expect("worker pool")
        } else {
            self.db.pool.clone()
        };
        let run_token = token.clone();
        let handle = tokio::spawn(async move {
            app::run_worker(config, pool, run_token)
                .await
                .expect("run worker");
        });
        self.workers.push((token, handle));
    }

    /// Stops the workers, letting running jobs return to the queue.
    pub async fn stop_workers(&mut self) {
        for (token, handle) in self.workers.drain(..) {
            token.cancel();
            let _ = handle.await;
        }
    }

    /// Stops the API and workers, leaving database cleanup to the caller.
    pub async fn stop_api(mut self) -> TestDb {
        self.stop_workers().await;
        self.shutdown.cancel();
        self.api.await.expect("API task");
        self.db
    }

    pub async fn stop(self) {
        self.stop_api().await.drop_database().await;
    }

    pub fn url(&self, path: &str) -> Url {
        self.base
            .join(path.trim_start_matches('/'))
            .expect("join URL")
    }

    pub async fn get(&self, auth: &Auth, path: &str) -> reqwest::Response {
        auth.apply(self.http.get(self.url(path)))
            .send()
            .await
            .expect("GET")
    }

    pub async fn post(&self, auth: &Auth, path: &str, body: &Value) -> reqwest::Response {
        auth.apply(self.http.post(self.url(path)).json(body))
            .send()
            .await
            .expect("POST")
    }

    pub async fn delete(&self, auth: &Auth, path: &str) -> reqwest::Response {
        auth.apply(self.http.delete(self.url(path)))
            .send()
            .await
            .expect("DELETE")
    }

    /// Creates an upload; returns the response JSON (with the presigned request).
    pub async fn create_upload(&self, auth: &Auth, filename: &str, bytes: &[u8]) -> Value {
        let response = self
            .post(
                auth,
                "/v1/uploads",
                &json!({
                    "filename": filename,
                    "size": bytes.len(),
                    "sha256": sha256_hex(bytes),
                }),
            )
            .await;
        assert_eq!(
            response.status(),
            201,
            "create upload: {}",
            text(response).await
        );
        response.json().await.expect("upload JSON")
    }

    /// Sends the bytes with the presigned request.
    pub async fn put_presigned(&self, upload: &Value, bytes: &[u8]) -> reqwest::Response {
        let presigned = &upload["upload"];
        let mut request = self
            .http
            .put(presigned["url"].as_str().expect("url"))
            .body(bytes.to_vec());
        for (name, value) in presigned["headers"].as_object().expect("headers") {
            if name != "content-length" {
                request = request.header(name.as_str(), value.as_str().expect("header"));
            }
        }
        request.send().await.expect("PUT")
    }

    /// Uploads and completes a file; returns the completed upload JSON.
    pub async fn upload(&self, auth: &Auth, filename: &str, bytes: &[u8]) -> Value {
        let upload = self.create_upload(auth, filename, bytes).await;
        let put = self.put_presigned(&upload, bytes).await;
        assert!(put.status().is_success(), "PUT: {}", text(put).await);
        let id = upload["id"].as_str().expect("id");
        let response = self
            .post(auth, &format!("/v1/uploads/{id}/complete"), &json!({}))
            .await;
        assert_eq!(response.status(), 200, "complete: {}", text(response).await);
        response.json().await.expect("upload JSON")
    }

    pub async fn create_job(&self, auth: &Auth, upload_id: &str, to: &str) -> reqwest::Response {
        self.post(
            auth,
            "/v1/jobs",
            &json!({ "upload_id": upload_id, "to": to }),
        )
        .await
    }

    /// Polls a job until it reaches a final status.
    pub async fn wait_final(&self, auth: &Auth, job_id: &str, timeout: Duration) -> Value {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let job: Value = self
                .get(auth, &format!("/v1/jobs/{job_id}"))
                .await
                .json()
                .await
                .expect("job JSON");
            let status = job["status"].as_str().unwrap_or_default().to_owned();
            if ["succeeded", "failed", "canceled", "expired"].contains(&status.as_str()) {
                return job;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "job {job_id} still {status} after {timeout:?}: {job}"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Files under the object store root.
    pub fn stored_files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, out);
                    } else {
                        out.push(path);
                    }
                }
            }
        }
        let mut files = Vec::new();
        walk(self.objects.path(), &mut files);
        files
    }

    pub async fn sql_exec(&self, sql: &'static str) {
        sqlx::query(sql).execute(&self.db.pool).await.expect(sql);
    }
}

pub async fn text(response: reqwest::Response) -> String {
    let status = response.status();
    format!("{status} {}", response.text().await.unwrap_or_default())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The `ashift` binary: `ARIAD_TEST_ENGINE_PROGRAM`, else the one next to
/// the test's target directory, else built once from this workspace. An
/// existing binary is reused rather than rebuilt so a workspace test run never
/// replaces the binary other crates' tests are executing.
pub fn ashift() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(path) = std::env::var_os("ARIAD_TEST_ENGINE_PROGRAM") {
            return PathBuf::from(path);
        }
        let exe = std::env::current_exe().expect("test executable");
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("target profile directory")
            .to_path_buf();
        let existing = profile_dir.join(format!("ashift{}", std::env::consts::EXE_SUFFIX));
        if existing.is_file() {
            return existing;
        }
        let profile = match profile_dir.file_name().and_then(|n| n.to_str()) {
            Some("debug") | None => "dev".to_owned(),
            Some(other) => other.to_owned(),
        };
        let target_dir = profile_dir.parent().expect("target directory");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "--quiet",
                "-p",
                "ariad-cli",
                "--bin",
                "ashift",
                "--profile",
            ])
            .arg(&profile)
            .arg("--target-dir")
            .arg(target_dir)
            .current_dir(workspace)
            .status()
            .expect("run cargo build");
        assert!(status.success(), "building ashift failed");
        profile_dir.join(format!("ashift{}", std::env::consts::EXE_SUFFIX))
    })
    .clone()
}

/// Reads an SSE response until the stream ends; returns `(event, data)` pairs.
pub async fn read_sse(response: reqwest::Response, timeout: Duration) -> Vec<(String, Value)> {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let next = tokio::time::timeout_at(deadline, stream.next())
            .await
            .expect("SSE stream did not finish in time");
        let Some(chunk) = next else { break };
        buffer.push_str(&String::from_utf8_lossy(&chunk.expect("SSE chunk")));
        while let Some(end) = buffer.find("\n\n") {
            let block: String = buffer.drain(..end + 2).collect();
            let mut kind = String::new();
            let mut data = String::new();
            for line in block.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    kind = v.trim().to_owned();
                } else if let Some(v) = line.strip_prefix("data:") {
                    data.push_str(v.trim());
                }
            }
            if !kind.is_empty() {
                events.push((kind, serde_json::from_str(&data).unwrap_or(Value::Null)));
            }
        }
    }
    events
}

/// A stand-in for Better Auth's JWT plugin: an Ed25519 key served as a JWKS.
pub struct TestIssuer {
    pub origin: String,
    pub jwks_url: String,
    key: EncodingKey,
    pub kid: String,
    shutdown: CancellationToken,
}

impl TestIssuer {
    pub async fn start() -> Self {
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&Sha256::digest(Uuid::new_v4().as_bytes()));
        // PKCS#8 v1 wrapping of an Ed25519 private key (RFC 8410).
        let mut der = hex::decode("302e020100300506032b657004220420").expect("prefix");
        der.extend_from_slice(&seed);
        let key = EncodingKey::from_ed_der(&der);
        let kid = Uuid::new_v4().simple().to_string();
        let mut jwk = Jwk::from_encoding_key(&key, Algorithm::EdDSA).expect("JWK");
        jwk.common.key_id = Some(kid.clone());
        let jwks = json!({ "keys": [jwk] });

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr: SocketAddr = listener.local_addr().expect("addr");
        let shutdown = CancellationToken::new();
        let app = Router::new().route(
            "/api/auth/jwks",
            get(move || {
                let jwks = jwks.clone();
                async move { Json(jwks) }
            }),
        );
        let stop = shutdown.clone();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await;
        });
        let origin = format!("http://{addr}");
        Self {
            jwks_url: format!("{origin}/api/auth/jwks"),
            origin,
            key,
            kid,
            shutdown,
        }
    }

    /// API environment that trusts this issuer.
    pub fn env(&self) -> Vec<(String, String)> {
        vec![("ARIAD_AUTH_URL".to_owned(), self.origin.clone())]
    }

    /// A token shaped like Better Auth's: `sub`, `iss`, `aud`, `iat`, `exp`, user fields.
    pub fn token(&self, sub: &str) -> String {
        self.token_with(sub, 900, &self.origin, &self.kid)
    }

    pub fn token_with(&self, sub: &str, expires_in: i64, audience: &str, kid: &str) -> String {
        let now = chrono::Utc::now().timestamp();
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(kid.to_owned());
        let claims = json!({
            "sub": sub,
            "id": sub,
            "email": format!("{sub}@example.test"),
            "iss": self.origin,
            "aud": audience,
            "iat": now,
            "exp": now + expires_in,
        });
        jsonwebtoken::encode(&header, &claims, &self.key).expect("sign")
    }

    pub fn stop(&self) {
        self.shutdown.cancel();
    }
}

/// A Turnstile `siteverify` stand-in accepting the token `good-token`.
pub async fn start_turnstile() -> (String, CancellationToken) {
    #[derive(serde::Deserialize)]
    struct Form {
        secret: String,
        response: String,
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = Router::new().route(
        "/siteverify",
        post(|axum::Form(form): axum::Form<Form>| async move {
            Json(json!({
                "success": form.secret == "turnstile-secret" && form.response == "good-token"
            }))
        }),
    );
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await;
    });
    (format!("http://{addr}/siteverify"), shutdown)
}

/// Creates Better Auth's `apikey` table exactly as its PostgreSQL migration
/// does (`@better-auth/api-key` 1.7.12). Test fixture only: Better Auth owns
/// this table in real deployments.
pub async fn create_apikey_table(pool: &PgPool) {
    sqlx::raw_sql(
        r#"create table "apikey" ("id" text not null primary key, "configId" text not null, "name" text, "start" text, "referenceId" text not null, "prefix" text, "key" text not null, "refillInterval" integer, "refillAmount" integer, "lastRefillAt" timestamptz, "enabled" boolean, "rateLimitEnabled" boolean, "rateLimitTimeWindow" integer, "rateLimitMax" integer, "requestCount" integer, "remaining" integer, "lastRequest" timestamptz, "expiresAt" timestamptz, "createdAt" timestamptz not null, "updatedAt" timestamptz not null, "permissions" text, "metadata" text);
           create index "apikey_configId_idx" on "apikey" ("configId");
           create index "apikey_referenceId_idx" on "apikey" ("referenceId");
           create index "apikey_key_idx" on "apikey" ("key");"#,
    )
    .execute(pool)
    .await
    .expect("create apikey");
}

/// Inserts a key row with Better Auth's defaults; returns the raw key.
pub async fn insert_api_key(pool: &PgPool, user_id: &str) -> String {
    let raw: String = (0..64)
        .map(|i| {
            let byte = Sha256::digest(format!("{user_id}-{i}-{}", Uuid::new_v4()).as_bytes())[0];
            char::from(b'a' + byte % 26)
        })
        .collect();
    sqlx::query(
        r#"insert into "apikey" ("id", "configId", "start", "referenceId", "key", "enabled",
             "rateLimitEnabled", "rateLimitTimeWindow", "rateLimitMax", "requestCount",
             "createdAt", "updatedAt", "metadata")
           values ($1, 'default', $2, $3, $4, true, true, 86400000, 10, 0, now(), now(), 'null')"#,
    )
    .bind(Uuid::new_v4().simple().to_string())
    .bind(&raw[..6])
    .bind(user_id)
    .bind(ariad_server::auth::api_key::hash_key(&raw))
    .execute(pool)
    .await
    .expect("insert key");
    raw
}
