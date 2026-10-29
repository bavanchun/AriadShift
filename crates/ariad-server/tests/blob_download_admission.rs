//! Admission after a consumer reads the last data chunk but leaves EOF queued.
use ariad_server::{
    config::{BlobTransferLimits, StorageConfig},
    storage::{Storage, StorageError},
};
use futures_util::{StreamExt, stream};
use sha2::{Digest, Sha256};
use std::time::Duration;

#[tokio::test]
async fn unread_eof_releases_the_download_slot() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::from_config(
        &StorageConfig::Local {
            root: dir.path().to_owned(),
        },
        None,
        None,
    )
    .unwrap()
    .with_blob_transfer_limits(BlobTransferLimits {
        max_concurrent: 1,
        idle_timeout: Duration::from_millis(200),
        transfer_timeout: Duration::from_secs(60),
    })
    .unwrap();
    let payload = b"last data chunk";
    assert!(
        storage
            .put_stream(
                "tail",
                stream::iter([Ok::<_, std::io::Error>(bytes::Bytes::from_static(payload))]),
                payload.len() as u64,
                &hex::encode(Sha256::digest(payload))
            )
            .await
            .unwrap()
    );
    let (_, mut body) = storage.get_stream("tail").await.unwrap();
    assert_eq!(body.next().await.unwrap().unwrap().as_ref(), payload);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match storage.get_stream("tail").await {
                Ok((_, next)) => {
                    drop(next);
                    break;
                }
                Err(StorageError::Busy) => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(err) => panic!("unexpected storage error: {err}"),
            }
        }
    })
    .await
    .expect("unread EOF must not hold admission until the transfer deadline");
    // Releasing admission must not discard the queued end of the response.
    assert!(body.next().await.is_none());
}
