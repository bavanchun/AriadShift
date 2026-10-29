//! Real database completion with observable, explicitly released backend reads.
mod common;

use ariad_server::{
    api::{self, AppState, Caller, Inner, uploads::complete_upload},
    auth::{Authenticator, Credential, Principal},
    config::{ApiConfig, Env},
    db::{self, NewUpload, UploadStatus},
    entitlements,
    packs::Pack,
    queue::PgmqQueue,
    storage::{Direction, Storage, StorageError},
    telemetry::Metrics,
};
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{Method, StatusCode},
    response::Response,
    routing::any,
};
use common::TestDb;
use futures_util::FutureExt;
use std::{sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(5);
const CONTENT: &[u8] = b"# Verified upload\n";

struct BackendRead {
    key: String,
    release: oneshot::Sender<()>,
}
#[derive(Clone)]
struct Backend {
    root: std::path::PathBuf,
    reads: mpsc::UnboundedSender<BackendRead>,
}

async fn object(
    State(backend): State<Backend>,
    Path(key): Path<String>,
    method: Method,
) -> Response {
    let path = backend.root.join(&key);
    if method == Method::DELETE {
        let _ = tokio::fs::remove_file(path).await;
        return Response::builder().status(204).body(Body::empty()).unwrap();
    }
    let Ok(bytes) = tokio::fs::read(path).await else {
        return Response::builder().status(404).body(Body::empty()).unwrap();
    };
    if method == Method::GET {
        let (release, wait) = oneshot::channel();
        backend.reads.send(BackendRead { key, release }).unwrap();
        let _ = wait.await;
    }
    Response::builder()
        .status(200)
        .header("content-length", bytes.len())
        .header("last-modified", "Tue, 27 Oct 2026 00:00:00 GMT")
        .header("etag", "\"test-object\"")
        .body(if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(bytes)
        })
        .unwrap()
}

struct Rig {
    db: TestDb,
    state: AppState,
    root: tempfile::TempDir,
    reads: mpsc::UnboundedReceiver<BackendRead>,
    server: JoinHandle<()>,
}
impl Rig {
    async fn start(name: &str) -> Option<Self> {
        let db = TestDb::create(name).await?;
        let version: String = sqlx::query_scalar("SELECT version()")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        eprintln!("upload_verification real database ({name}): {version}");
        let root = tempfile::tempdir().unwrap();
        let (sender, reads) = mpsc::unbounded_channel();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/bucket/{*key}", any(object))
            .with_state(Backend {
                root: root.path().to_owned(),
                reads: sender,
            });
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let config = ApiConfig::from_env(&Env::from_pairs([
            ("ARIAD_SECRET", common::SECRET.as_str()),
            ("ARIAD_STORAGE_URL", "s3://bucket"),
            ("AWS_ENDPOINT", &endpoint),
            ("AWS_ALLOW_HTTP", "true"),
            // This backend implements single-object DELETE, not batch deletion.
            ("AWS_DISABLE_BULK_DELETE", "true"),
            ("AWS_REGION", "us-east-1"),
            ("AWS_ACCESS_KEY_ID", "test"),
            ("AWS_SECRET_ACCESS_KEY", "test"),
        ]))
        .unwrap();
        let storage = Storage::from_config(&config.storage, None, None).unwrap();
        let state = AppState::new(Inner {
            db: db.pool.clone(),
            storage,
            queue: Arc::new(PgmqQueue::new(db.pool.clone())),
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
        Some(Self {
            db,
            state,
            root,
            reads,
            server,
        })
    }
    async fn caller(&self) -> Caller {
        Caller {
            principal: Principal {
                credential: Credential::Anonymous,
                user_id: None,
                workspace_id: db::ANONYMOUS_WORKSPACE,
                plan: db::ANONYMOUS_PLAN.to_owned(),
                subject: "ip:verification-test".to_owned(),
                client: "verification-test".to_owned(),
            },
            entitlement: entitlements::load(&self.db.pool, db::ANONYMOUS_PLAN)
                .await
                .unwrap(),
            peer: None,
        }
    }
    async fn upload(&self, content: Option<&[u8]>, hash: &str) -> (Uuid, String) {
        let id = Uuid::new_v4();
        let key = self
            .state
            .storage
            .key(db::ANONYMOUS_WORKSPACE, id, Direction::In, hash);
        if let Some(content) = content {
            let path = self.root.path().join(&key);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let mut tx = self.db.pool.begin().await.unwrap();
        db::insert_upload(
            &mut tx,
            &NewUpload {
                id,
                workspace_id: db::ANONYMOUS_WORKSPACE,
                subject: "ip:verification-test",
                filename: "document.md",
                size_bytes: CONTENT.len() as i64,
                sha256: hash,
                storage_key: &key,
                retention_s: 1200,
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        (id, key)
    }
    async fn complete(
        &self,
        id: Uuid,
    ) -> JoinHandle<Result<axum::Json<api::dto::Upload>, ariad_server::error::ApiError>> {
        let state = self.state.clone();
        let caller = self.caller().await;
        tokio::spawn(complete_upload(State(state), caller, Path(id.to_string())))
    }
    async fn read(&mut self) -> BackendRead {
        timeout(WAIT, self.reads.recv()).await.unwrap().unwrap()
    }
    async fn stop(self) {
        self.server.abort();
        let _ = self.server.await;
        self.db.drop_database().await;
    }
}

#[tokio::test]
async fn overlapping_completion_reads_the_backend_only_once() {
    let Some(mut rig) = Rig::start("verification_overlap").await else {
        return;
    };
    let (id, key) = rig
        .upload(Some(CONTENT), &common::sha256_hex(CONTENT))
        .await;
    let first = rig.complete(id).await;
    let read = rig.read().await;
    assert_eq!(read.key, key);
    let overlap = timeout(
        WAIT,
        complete_upload(
            State(rig.state.clone()),
            rig.caller().await,
            Path(id.to_string()),
        ),
    )
    .await
    .unwrap();
    assert_eq!(overlap.unwrap_err().status(), StatusCode::CONFLICT);
    assert!(rig.reads.try_recv().is_err());
    read.release.send(()).unwrap();
    assert_eq!(
        timeout(WAIT, first)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .0
            .status,
        UploadStatus::Completed
    );
    assert_eq!(
        complete_upload(
            State(rig.state.clone()),
            rig.caller().await,
            Path(id.to_string()),
        )
        .await
        .unwrap()
        .0
        .status,
        UploadStatus::Completed
    );
    assert!(rig.reads.try_recv().is_err());
    rig.stop().await;
}

#[tokio::test]
async fn queued_completion_rechecks_settled_expired_deleted_missing_and_access() {
    let Some(mut rig) = Rig::start("verification_recheck").await else {
        return;
    };
    let mut permits = Vec::new();
    for n in 0..4 {
        permits.push(
            rig.state
                .storage
                .claim_digest(&format!("held-{n}"))
                .await
                .unwrap(),
        );
    }
    for change in [
        "completed",
        "expired",
        "expired-status",
        "deleted",
        "missing",
        "access",
    ] {
        let (id, key) = rig
            .upload(Some(CONTENT), &common::sha256_hex(CONTENT))
            .await;
        let future = complete_upload(
            State(rig.state.clone()),
            rig.caller().await,
            Path(id.to_string()),
        );
        tokio::pin!(future);
        // Poll the real handler until its claim exists while all permits remain held.
        timeout(WAIT, async {
            loop {
                assert!(future.as_mut().now_or_never().is_none());
                match rig.state.storage.claim_digest(&key).now_or_never() {
                    Some(Err(StorageError::DigestInProgress)) => break,
                    None => tokio::task::yield_now().await,
                    _ => panic!("unexpected claim result"),
                }
            }
        })
        .await
        .unwrap();
        match change {
            "completed" => {
                db::complete_upload(&rig.db.pool, id, Some("md"), 1200)
                    .await
                    .unwrap();
            }
            "expired" => {
                sqlx::query(
                    "UPDATE uploads SET expires_at = now() - interval '1 second' WHERE id = $1",
                )
                .bind(id)
                .execute(&rig.db.pool)
                .await
                .unwrap();
            }
            "expired-status" => {
                let mut conn = rig.db.pool.acquire().await.unwrap();
                db::retire_upload(&mut conn, id, UploadStatus::Expired)
                    .await
                    .unwrap();
            }
            "deleted" => {
                let mut conn = rig.db.pool.acquire().await.unwrap();
                db::retire_upload(&mut conn, id, UploadStatus::Deleted)
                    .await
                    .unwrap();
            }
            "missing" => {
                sqlx::query("DELETE FROM uploads WHERE id = $1")
                    .bind(id)
                    .execute(&rig.db.pool)
                    .await
                    .unwrap();
            }
            "access" => {
                let workspace = Uuid::new_v4();
                sqlx::query("INSERT INTO workspaces (id, owner_user_id, plan) VALUES ($1, 'other-user', 'free')").bind(workspace).execute(&rig.db.pool).await.unwrap();
                sqlx::query("UPDATE uploads SET workspace_id = $2 WHERE id = $1")
                    .bind(id)
                    .bind(workspace)
                    .execute(&rig.db.pool)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(permits.pop());
        let result = timeout(WAIT, future).await.unwrap();
        if change == "completed" {
            assert_eq!(result.unwrap().0.status, UploadStatus::Completed);
        } else {
            let expected = if ["missing", "access"].contains(&change) {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::CONFLICT
            };
            assert_eq!(result.unwrap_err().status(), expected, "{change}");
        }
        assert!(
            rig.reads.try_recv().is_err(),
            "settled upload must not read storage"
        );
        permits.push(rig.state.storage.claim_digest("replacement").await.unwrap());
    }
    drop(permits);
    rig.stop().await;
}

#[tokio::test]
async fn verification_failures_and_active_cancellation_release_claims() {
    let Some(mut rig) = Rig::start("verification_failure_cancel").await else {
        return;
    };
    let (missing, key) = rig.upload(None, &common::sha256_hex(CONTENT)).await;
    let err = rig.complete(missing).await.await.unwrap().unwrap_err();
    assert_eq!(err.status(), StatusCode::CONFLICT);
    drop(rig.state.storage.claim_digest(&key).await.unwrap());
    let (bad, key) = rig.upload(Some(CONTENT), &"0".repeat(64)).await;
    let failed = rig.complete(bad).await;
    rig.read().await.release.send(()).unwrap();
    assert_eq!(
        failed.await.unwrap().unwrap_err().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    drop(rig.state.storage.claim_digest(&key).await.unwrap());
    assert!(!rig.root.path().join(key).exists());
    let (id, key) = rig
        .upload(Some(CONTENT), &common::sha256_hex(CONTENT))
        .await;
    let mut permits = Vec::new();
    for n in 0..4 {
        permits.push(
            rig.state
                .storage
                .claim_digest(&format!("cancel-held-{n}"))
                .await
                .unwrap(),
        );
    }
    {
        let mut waiting = Box::pin(complete_upload(
            State(rig.state.clone()),
            rig.caller().await,
            Path(id.to_string()),
        ));
        timeout(WAIT, async {
            loop {
                assert!(waiting.as_mut().now_or_never().is_none());
                if matches!(
                    rig.state.storage.claim_digest(&key).now_or_never(),
                    Some(Err(StorageError::DigestInProgress))
                ) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    drop(permits.pop());
    drop(rig.state.storage.claim_digest(&key).await.unwrap());
    drop(permits);
    assert!(rig.reads.try_recv().is_err());
    let active = rig.complete(id).await;
    let read = rig.read().await;
    active.abort();
    assert!(active.await.unwrap_err().is_cancelled());
    drop(rig.state.storage.claim_digest(&key).await.unwrap());
    let _ = read.release.send(());
    let retry = rig.complete(id).await;
    rig.read().await.release.send(()).unwrap();
    assert_eq!(
        retry.await.unwrap().unwrap().0.status,
        UploadStatus::Completed
    );
    rig.stop().await;
}
