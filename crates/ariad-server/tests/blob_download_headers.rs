//! Real local blob route attachment headers and unchanged response bytes.
#[path = "common/signing_key.rs"]
mod signing_key;

use ariad_server::{
    api::{self, AppState, Inner, RouterOptions},
    auth::Authenticator,
    config::{ApiConfig, Env, StorageConfig},
    packs::Pack,
    queue::PgmqQueue,
    storage::Storage,
    telemetry::Metrics,
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};
use tower::ServiceExt;
use url::Url;
static SIGNING_KEY: LazyLock<String> =
    LazyLock::new(|| hex::encode(signing_key::random_signing_key()));
const WAIT: Duration = Duration::from_secs(5);
fn app(storage: Storage) -> axum::Router {
    let config = ApiConfig::from_env(&Env::from_pairs([
        ("ARIAD_SECRET", SIGNING_KEY.as_str()),
        ("ARIAD_STORAGE_URL", "s3://unused"),
    ]))
    .unwrap();
    // Blob routes do not access the database. A lazy real pool makes any
    // accidental dependency on it fail instead of silently mocking it.
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    let state = AppState::new(Inner {
        queue: Arc::new(PgmqQueue::new(db.clone())),
        db,
        storage,
        auth: Authenticator::new(
            config.auth,
            config.secret,
            None,
            ariad_server::http_client(),
        ),
        events: api::events::EventHub::new(),
        packs: vec![Pack::Core],
        metrics: Metrics::new(),
    });
    api::router(
        state,
        &RouterOptions {
            cors_origins: vec![],
            request_timeout: WAIT,
        },
    )
}

#[tokio::test]
async fn downloaded_objects_are_attachments_without_sniffing_or_caching() {
    let dir = tempfile::tempdir().unwrap();
    let payload = b"<script>untrusted document</script>";
    std::fs::write(dir.path().join("document"), payload).unwrap();
    let storage = Storage::from_config(
        &StorageConfig::Local {
            root: dir.path().to_owned(),
        },
        Some(&Url::parse("http://localhost:8080").unwrap()),
        Some(SIGNING_KEY.as_bytes()),
    )
    .unwrap();
    let (url, _) = storage.presign_get("document").await.unwrap();
    let response = app(storage)
        .oneshot(Request::get(url.path()).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["content-disposition"], "attachment");
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["content-type"], "application/octet-stream");
    assert_eq!(headers["content-length"], payload.len().to_string());
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        payload
    );
}
