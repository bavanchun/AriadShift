//! Local blob resource budgets, without a database or external services.

use std::{
    convert::Infallible,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ariad_server::{
    api::{self, AppState, Inner, RouterOptions},
    auth::Authenticator,
    config::{ApiConfig, BlobTransferLimits, Env, StorageConfig},
    packs::Pack,
    queue::PgmqQueue,
    storage::{Storage, StorageError},
    telemetry::Metrics,
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use bytes::Bytes;
use chrono::Utc;
use futures_util::{Stream, StreamExt, stream};
use sha2::{Digest, Sha256};
use tokio::{sync::oneshot, time::timeout};
use tower::ServiceExt;
use url::Url;

const WAIT: Duration = Duration::from_secs(5);
const PAYLOAD: &[u8] = b"# Uploaded\n";

fn storage(dir: &Path, count: usize, idle: Duration, total: Duration) -> Storage {
    Storage::from_config(
        &StorageConfig::Local {
            root: dir.to_owned(),
        },
        Some(&Url::parse("http://localhost:8080").unwrap()),
        Some(b"0123456789abcdef0123456789abcdef"),
    )
    .unwrap()
    .with_blob_transfer_limits(BlobTransferLimits {
        max_concurrent: count,
        idle_timeout: idle,
        transfer_timeout: total,
    })
    .unwrap()
}

fn app(storage: Storage) -> axum::Router {
    let config = ApiConfig::from_env(&Env::from_pairs([
        ("ARIAD_SECRET", "0123456789abcdef0123456789abcdef"),
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

fn chunks(data: &'static [u8]) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin {
    stream::iter([Ok(Bytes::from_static(data))])
}

fn held_body() -> (Body, oneshot::Receiver<()>) {
    let (sender, observed) = oneshot::channel();
    let mut sender = Some(sender);
    let mut first = true;
    let body = stream::poll_fn(move |_| {
        if first {
            first = false;
            return std::task::Poll::Ready(Some(Ok::<_, Infallible>(Bytes::from_static(b"#"))));
        }
        if let Some(sender) = sender.take() {
            let _ = sender.send(());
        }
        std::task::Poll::Pending
    });
    (Body::from_stream(body), observed)
}

async fn put_path(storage: &Storage, key: &str) -> String {
    storage
        .presign_put(
            key,
            PAYLOAD.len() as u64,
            &hex::encode(Sha256::digest(PAYLOAD)),
        )
        .await
        .unwrap()
        .url
        .path()
        .to_owned()
}

async fn assert_no_files(root: &Path) {
    fn count(root: &Path) -> usize {
        std::fs::read_dir(root)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                if path.is_dir() { count(&path) } else { 1 }
            })
            .sum()
    }
    // Local multipart Drop cleanup runs on the runtime's blocking pool.
    timeout(WAIT, async {
        while count(root) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("staging files were cleaned up");
}

async fn assert_slot_released(storage: &Storage) {
    timeout(WAIT, async {
        loop {
            match storage.get_stream("absent").await {
                Err(StorageError::Busy) => tokio::task::yield_now().await,
                Err(err) if err.is_not_found() => break,
                _ => panic!("expected a released slot and a missing object"),
            }
        }
    })
    .await
    .unwrap();
}

async fn round_trip(storage: &Storage, key: &str) {
    let hash = hex::encode(Sha256::digest(PAYLOAD));
    assert!(
        !storage
            .put_stream(key, chunks(PAYLOAD), PAYLOAD.len() as u64, &"0".repeat(64))
            .await
            .unwrap()
    );
    assert!(storage.size(key).await.unwrap().is_none());
    assert!(
        storage
            .put_stream(key, chunks(PAYLOAD), PAYLOAD.len() as u64, &hash)
            .await
            .unwrap()
    );
    let digest = storage.digest(key, 1024).await.unwrap();
    assert_eq!(digest.size, PAYLOAD.len() as u64);
    assert_eq!(digest.sha256, hash);
    storage.delete(key).await.unwrap();
}

#[tokio::test]
async fn replay_and_global_overload_are_refused_before_polling_the_body() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path(), 2, WAIT, WAIT);
    let router = app(storage.clone());
    let path = put_path(&storage, "first").await;
    let (body, observed) = held_body();
    let first = tokio::spawn(
        router
            .clone()
            .oneshot(Request::put(&path).body(body).unwrap()),
    );
    timeout(WAIT, observed).await.unwrap().unwrap();
    let never_polled = Arc::new(AtomicBool::new(false));
    let request = |path: String| {
        let polled = Arc::clone(&never_polled);
        let body = Body::from_stream(stream::poll_fn(move |_| {
            polled.store(true, Ordering::SeqCst);
            std::task::Poll::Ready(None::<Result<Bytes, Infallible>>)
        }));
        Request::put(path).body(body).unwrap()
    };
    let replay = router.clone().oneshot(request(path.clone())).await.unwrap();
    assert_eq!(replay.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(replay.headers()["retry-after"], "1");
    assert!(!never_polled.load(Ordering::SeqCst));
    let second_path = put_path(&storage, "second").await;
    let (body, observed) = held_body();
    let second = tokio::spawn(
        router
            .clone()
            .oneshot(Request::put(second_path).body(body).unwrap()),
    );
    timeout(WAIT, observed).await.unwrap().unwrap();
    let other = router
        .clone()
        .oneshot(request(put_path(&storage, "third").await))
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(!never_polled.load(Ordering::SeqCst));
    let (get_url, _) = storage.presign_get("not-yet-stored").await.unwrap();
    let get = router
        .clone()
        .oneshot(Request::get(get_url.path()).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::TOO_MANY_REQUESTS);
    first.abort();
    second.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(second.await.unwrap_err().is_cancelled());
    assert_no_files(dir.path()).await;
    let response = router
        .clone()
        .oneshot(Request::put(path).body(Body::from(PAYLOAD)).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        storage.digest("first", 1024).await.unwrap().sha256,
        hex::encode(Sha256::digest(PAYLOAD))
    );
    let (url, _) = storage.presign_get("first").await.unwrap();
    let get = router
        .oneshot(Request::get(url.path()).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    assert_eq!(get.headers()["content-type"], "application/octet-stream");
    assert_eq!(get.headers()["content-length"], PAYLOAD.len().to_string());
    assert_eq!(get.headers()["cache-control"], "no-store");
    assert_eq!(
        to_bytes(get.into_body(), 1024).await.unwrap().as_ref(),
        PAYLOAD
    );
}

#[tokio::test]
async fn idle_timeout_cleans_staging_and_releases_admission() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path(), 1, Duration::from_millis(100), WAIT);
    let router = app(storage.clone());
    let path = put_path(&storage, "idle").await;
    let (body, _) = held_body();
    let response = timeout(WAIT, router.oneshot(Request::put(path).body(body).unwrap()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("transfer_timeout"));
    assert_no_files(dir.path()).await;
    round_trip(&storage, "idle").await;
}

#[tokio::test]
async fn total_deadline_stops_dripping_uploads_and_releases_admission() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path(), 1, WAIT, Duration::from_millis(250));
    let body = stream::unfold((), |()| async {
        tokio::time::sleep(Duration::from_millis(1)).await;
        Some((Ok::<_, Infallible>(Bytes::from_static(b"x")), ()))
    })
    .boxed();
    let result = timeout(
        WAIT,
        storage.put_stream("drip", body, u64::MAX, &"0".repeat(64)),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(StorageError::Timeout)));
    assert_no_files(dir.path()).await;
    round_trip(&storage, "drip").await;
}

#[tokio::test]
async fn accepted_upload_stops_at_url_expiry_and_expired_replay_is_forbidden() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path(), 1, WAIT, WAIT);
    let signed = storage
        .presign_put(
            "expires",
            PAYLOAD.len() as u64,
            &hex::encode(Sha256::digest(PAYLOAD)),
        )
        .await
        .unwrap();
    let token = signed.url.path().trim_start_matches("/v1/blobs/");
    let mut claims = storage.verify_blob_token(token, Utc::now()).unwrap();
    claims.expires = Utc::now().timestamp() + 2;
    let path = signed_path(&claims);
    let router = app(storage.clone());
    let (body, observed) = held_body();
    let request = tokio::spawn(
        router
            .clone()
            .oneshot(Request::put(&path).body(body).unwrap()),
    );
    timeout(WAIT, observed).await.unwrap().unwrap();
    let response = timeout(WAIT, request).await.unwrap().unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert!(Utc::now().timestamp() >= claims.expires);
    assert_no_files(dir.path()).await;
    round_trip(&storage, "expires").await;
    let polled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&polled);
    let body = Body::from_stream(stream::poll_fn(move |_| {
        flag.store(true, Ordering::SeqCst);
        std::task::Poll::Ready(None::<Result<Bytes, Infallible>>)
    }));
    let response = router
        .oneshot(Request::put(path).body(body).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!polled.load(Ordering::SeqCst));
}

fn signed_path(claims: &ariad_server::storage::BlobClaims) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use hmac::{KeyInit, Mac};
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
    let mut mac =
        hmac::Hmac::<Sha256>::new_from_slice(b"0123456789abcdef0123456789abcdef").unwrap();
    mac.update(payload.as_bytes());
    format!(
        "/v1/blobs/{payload}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

#[tokio::test]
async fn body_errors_and_size_mismatch_release_upload_keys() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path(), 1, WAIT, WAIT);
    let failed = stream::iter([
        Ok(Bytes::from_static(b"x")),
        Err(std::io::Error::other("disconnected")),
    ]);
    assert!(
        storage
            .put_stream("failed", failed, 2, &"0".repeat(64))
            .await
            .is_err()
    );
    assert_no_files(dir.path()).await;
    assert!(
        !storage
            .put_stream("failed", chunks(PAYLOAD), 1, &"0".repeat(64))
            .await
            .unwrap()
    );
    assert_no_files(dir.path()).await;
    round_trip(&storage, "failed").await;
}

#[tokio::test]
async fn get_holds_shared_admission_and_releases_on_drop_or_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path(), 1, WAIT, WAIT);
    // Several backend chunks exceed the one-slot response channel.
    let payload = Bytes::from(vec![b'x'; 3 * 1024 * 1024]);
    let hash = hex::encode(Sha256::digest(&payload));
    let body = stream::iter([Ok::<_, Infallible>(payload.clone())]);
    assert!(
        storage
            .put_stream("download", body, payload.len() as u64, &hash)
            .await
            .unwrap()
    );
    let (_, stream) = storage.get_stream("download").await.unwrap();
    assert!(matches!(
        storage.get_stream("download").await,
        Err(StorageError::Busy)
    ));
    drop(stream);
    timeout(WAIT, async {
        loop {
            match storage.get_stream("download").await {
                Ok((_, stream)) => {
                    drop(stream);
                    break;
                }
                Err(StorageError::Busy) => tokio::task::yield_now().await,
                Err(err) => panic!("{err}"),
            }
        }
    })
    .await
    .unwrap();
    // An unpolled response must also release admission at its deadline.
    let expires: chrono::DateTime<Utc> = Utc::now() + chrono::Duration::milliseconds(20);
    let mut stream = timeout(WAIT, async {
        loop {
            match storage.get_stream_until("download", Some(expires)).await {
                Ok((_, stream)) => break stream,
                Err(StorageError::Busy) => tokio::task::yield_now().await,
                Err(err) => panic!("{err}"),
            }
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_slot_released(&storage).await;
    round_trip(&storage, "after-get").await;
    while stream.next().await.is_some() {}
    storage.delete("download").await.unwrap();
    assert_no_files(dir.path()).await;
}

#[tokio::test]
async fn idle_backpressure_on_a_small_download_releases_admission() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path(), 1, Duration::from_millis(100), WAIT);
    let hash = hex::encode(Sha256::digest(PAYLOAD));
    assert!(
        storage
            .put_stream("small", chunks(PAYLOAD), PAYLOAD.len() as u64, &hash)
            .await
            .unwrap()
    );
    let (_, stream) = storage.get_stream("small").await.unwrap();
    assert!(matches!(
        storage.get_stream("small").await,
        Err(StorageError::Busy)
    ));
    // The single data chunk fills the queue and EOF cannot be sent until
    // the idle timeout. Keep the response unpolled throughout this check.
    assert_slot_released(&storage).await;
    drop(stream);
    round_trip(&storage, "after-small").await;
    storage.delete("small").await.unwrap();
    assert_no_files(dir.path()).await;
}

#[test]
fn transfer_limits_cannot_be_disabled_with_zero_values() {
    for limits in [
        BlobTransferLimits {
            max_concurrent: 0,
            ..BlobTransferLimits::default()
        },
        BlobTransferLimits {
            idle_timeout: Duration::ZERO,
            ..BlobTransferLimits::default()
        },
        BlobTransferLimits {
            transfer_timeout: Duration::ZERO,
            ..BlobTransferLimits::default()
        },
    ] {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            storage(dir.path(), 1, WAIT, WAIT)
                .with_blob_transfer_limits(limits)
                .is_err()
        );
    }
}

#[test]
fn blob_openapi_declares_timeout_and_busy_responses() {
    let doc: serde_json::Value = serde_json::from_str(&api::openapi_json().unwrap()).unwrap();
    for method in ["put", "get"] {
        let responses = &doc["paths"]["/v1/blobs/{token}"][method]["responses"];
        assert!(responses["408"].is_object());
        assert!(responses["429"]["headers"]["retry-after"].is_object());
    }
}
