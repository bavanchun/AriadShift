//! Verification claims are shared across storage clones and cancellation safe.
use ariad_server::{
    config::StorageConfig,
    storage::{MAX_CONCURRENT_DIGESTS, Storage, StorageError},
};
use futures_util::FutureExt;
use std::time::Duration;

fn storage(root: &std::path::Path) -> Storage {
    Storage::from_config(
        &StorageConfig::Local {
            root: root.to_owned(),
        },
        None,
        None,
    )
    .unwrap()
}

#[tokio::test]
async fn claims_are_unique_and_the_fifth_distinct_object_waits() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path());
    assert_eq!(MAX_CONCURRENT_DIGESTS, 4);
    let first = storage.claim_digest("first").await.unwrap();
    assert!(matches!(
        storage.clone().claim_digest("first").await,
        Err(StorageError::DigestInProgress)
    ));
    let mut claims = Vec::new();
    for n in 1..MAX_CONCURRENT_DIGESTS {
        claims.push(storage.claim_digest(&format!("other-{n}")).await.unwrap());
    }
    let fifth = storage.claim_digest("fifth");
    tokio::pin!(fifth);
    assert!(fifth.as_mut().now_or_never().is_none());
    assert!(matches!(
        storage.claim_digest("fifth").await,
        Err(StorageError::DigestInProgress)
    ));
    drop(first);
    let fifth = tokio::time::timeout(Duration::from_secs(1), fifth)
        .await
        .unwrap()
        .unwrap();
    drop(fifth);
    drop(storage.claim_digest("fifth").await.unwrap());
}

#[tokio::test]
async fn cancellation_while_waiting_and_active_releases_keys_and_permits() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage(dir.path());
    let mut claims = Vec::new();
    for n in 0..MAX_CONCURRENT_DIGESTS {
        claims.push(storage.claim_digest(&n.to_string()).await.unwrap());
    }
    {
        let mut waiting = Box::pin(storage.claim_digest("waiting"));
        assert!(waiting.as_mut().now_or_never().is_none());
    }
    drop(claims.pop());
    let active = storage.claim_digest("waiting").await.unwrap();
    drop(active);
    drop(storage.claim_digest("waiting").await.unwrap());
    drop(claims);
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn({
        let storage = storage.clone();
        async move {
            let _claim = storage.claim_digest("active").await.unwrap();
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        }
    });
    started_rx.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(storage.claim_digest("active").await.unwrap());
}

#[tokio::test]
async fn digest_success_and_failure_release_the_claim_on_return() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("valid"), b"data").unwrap();
    let storage = storage(dir.path());
    for key in ["valid", "missing"] {
        let result = async {
            let _claim = storage.claim_digest(key).await.unwrap();
            storage.digest(key, 4).await
        }
        .await;
        assert_eq!(result.is_ok(), key == "valid");
        drop(storage.claim_digest(key).await.unwrap());
    }
}
