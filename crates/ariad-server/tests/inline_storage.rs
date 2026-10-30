//! Inline conversion remains available when HTTP blob transfer slots are full.

mod common;

use std::{convert::Infallible, time::Duration};

use ariad_server::{
    api::{AppState, Inner},
    app,
    config::{ApiConfig, BlobTransferLimits, Env},
    storage::StorageError,
};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use bytes::Bytes;
use common::{Harness, Options, create_apikey_table, insert_api_key};
use futures_util::stream;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread")]
async fn inline_conversion_succeeds_while_the_blob_budget_is_full() {
    let Some(h) = Harness::start("inline_saturation", Options::default()).await else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let key = insert_api_key(&h.db.pool, "alice").await;
    let config = ApiConfig::from_env(&Env::from_pairs([
        ("ARIAD_SECRET".to_owned(), common::SECRET.to_string()),
        ("ARIAD_PUBLIC_URL".to_owned(), h.base.to_string()),
        (
            "ARIAD_STORAGE_URL".to_owned(),
            url::Url::from_directory_path(h.objects.path())
                .unwrap()
                .to_string(),
        ),
    ]))
    .unwrap();
    let original = app::api_state(&config, h.db.pool.clone()).unwrap();
    let storage = original
        .storage
        .clone()
        .with_blob_transfer_limits(BlobTransferLimits {
            max_concurrent: 1,
            idle_timeout: Duration::from_secs(30),
            transfer_timeout: Duration::from_secs(30),
        })
        .unwrap();
    let held = Bytes::from_static(b"held");
    let hash = hex::encode(Sha256::digest(&held));
    let body = || stream::iter([Ok::<_, Infallible>(held.clone())]);
    assert!(storage.put_stream("held", body(), 4, &hash).await.unwrap());
    let (_, download) = storage.get_stream("held").await.unwrap();
    assert!(matches!(
        storage.put_stream("blocked", body(), 4, &hash).await,
        Err(StorageError::Busy)
    ));
    let state = AppState::new(Inner {
        db: original.db.clone(),
        storage: storage.clone(),
        queue: original.queue.clone(),
        auth: ariad_server::auth::Authenticator::new(
            config.auth.clone(),
            config.secret.clone(),
            None,
            ariad_server::http_client(),
        ),
        events: original.events.clone(),
        packs: original.packs.clone(),
        metrics: original.metrics.clone(),
    });
    let shutdown = CancellationToken::new();
    let router = ariad_server::mcp::router(state, &config, shutdown.clone());
    let payload = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":"convert","arguments":{"inline":{"filename":"notes.md","content":"# Inline\n"},"to":"html"},
        "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28", "io.modelcontextprotocol/clientInfo":{"name":"ariad-integration","version":"1"}, "io.modelcontextprotocol/clientCapabilities":{}}
    }});
    let response = router
        .oneshot(
            Request::post("/mcp")
                .header(
                    "host",
                    &h.base[url::Position::BeforeHost..url::Position::AfterPort],
                )
                .header("x-api-key", key)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "tools/call")
                .header("mcp-name", "convert")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_ne!(response["result"]["isError"], true, "{response}");
    let statuses: Vec<String> = sqlx::query_scalar("SELECT status FROM uploads")
        .fetch_all(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(statuses, ["completed"]);
    // Completion also bypasses admission; the original download still holds it.
    assert!(matches!(
        storage.put_stream("blocked", body(), 4, &hash).await,
        Err(StorageError::Busy)
    ));
    drop(download);
    shutdown.cancel();
    h.stop().await;
}

#[tokio::test]
async fn in_memory_put_does_not_consume_a_blob_transfer_slot() {
    use ariad_server::{config::StorageConfig, storage::Storage};

    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::from_config(
        &StorageConfig::Local {
            root: dir.path().to_owned(),
        },
        Some(&url::Url::parse("http://localhost:8080").unwrap()),
        Some(common::SECRET.as_bytes()),
    )
    .unwrap()
    .with_blob_transfer_limits(BlobTransferLimits {
        max_concurrent: 1,
        idle_timeout: Duration::from_secs(30),
        transfer_timeout: Duration::from_secs(30),
    })
    .unwrap();
    storage
        .put_bytes("held", Bytes::from_static(b"held"))
        .await
        .unwrap();
    let (_, download) = storage.get_stream("held").await.unwrap();
    let data = Bytes::from_static(b"# Inline\n");
    let hash = hex::encode(Sha256::digest(&data));
    assert!(matches!(
        storage
            .put_stream(
                "blocked",
                stream::iter([Ok::<_, Infallible>(data.clone())]),
                data.len() as u64,
                &hash
            )
            .await,
        Err(StorageError::Busy)
    ));
    storage.put_bytes("inline", data.clone()).await.unwrap();
    let actual = storage.digest("inline", data.len() as u64).await.unwrap();
    assert_eq!(actual.sha256, hash);
    assert_eq!(actual.prefix, data);
    drop(download);
}

#[tokio::test]
async fn in_memory_put_round_trips_through_the_real_s3_store() {
    use ariad_server::{config::StorageConfig, storage::Storage};

    ariad_server::install_crypto_provider();
    let Ok(endpoint) = std::env::var("ARIAD_TEST_S3_ENDPOINT") else {
        eprintln!(
            "skipping in_memory_put_round_trips_through_the_real_s3_store: ARIAD_TEST_S3_ENDPOINT is not set"
        );
        return;
    };
    let config = StorageConfig::from_env(&Env::from_pairs([
        ("ARIAD_STORAGE_URL", "s3://ariad-test"),
        ("AWS_ENDPOINT", endpoint.as_str()),
        ("AWS_ACCESS_KEY_ID", "ariadshift-test"),
        ("AWS_SECRET_ACCESS_KEY", "ariadshift-test-secret"),
        ("AWS_REGION", "us-east-1"),
        ("AWS_ALLOW_HTTP", "true"),
    ]))
    .unwrap();
    let storage = Storage::from_config(&config, None, None).unwrap();
    let key = format!("inline-{}", uuid::Uuid::new_v4());
    let data = Bytes::from(vec![b'a'; 700_000]);
    storage.put_bytes(&key, data.clone()).await.unwrap();
    let actual = storage.digest(&key, data.len() as u64).await.unwrap();
    assert_eq!(actual.size, data.len() as u64);
    assert_eq!(actual.sha256, hex::encode(Sha256::digest(&data)));
    storage.delete(&key).await.unwrap();
    assert_eq!(storage.size(&key).await.unwrap(), None);
}
