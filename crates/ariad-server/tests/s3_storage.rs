//! S3-compatible storage (SeaweedFS in CI and `just test-server`).
//!
//! Needs `ARIAD_TEST_S3_ENDPOINT` (and a bucket, `ariad-test` by default);
//! the job test also needs `ARIAD_TEST_DATABASE_URL`. Both skip otherwise.

mod common;

use std::time::Duration;

use ariad_server::{
    config::{Env, StorageConfig},
    storage::{CHECKSUM_HEADER, Storage},
};
use common::{Auth, Harness, Options, sha256_hex, text};
use serde_json::{Value, json};
use uuid::Uuid;

fn s3_env(test: &str) -> Option<Vec<(String, String)>> {
    let Some(endpoint) = std::env::var("ARIAD_TEST_S3_ENDPOINT")
        .ok()
        .filter(|v| !v.trim().is_empty())
    else {
        eprintln!("skipping {test}: ARIAD_TEST_S3_ENDPOINT is not set");
        return None;
    };
    let var =
        |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_owned());
    let bucket = var("ARIAD_TEST_S3_BUCKET", "ariad-test");
    Some(vec![
        (
            "ARIAD_STORAGE_URL".to_owned(),
            format!("s3://{bucket}/run-{}", Uuid::new_v4().simple()),
        ),
        ("AWS_ENDPOINT".to_owned(), endpoint),
        (
            "AWS_ACCESS_KEY_ID".to_owned(),
            var("ARIAD_TEST_S3_ACCESS_KEY", "ariadshift-test"),
        ),
        (
            "AWS_SECRET_ACCESS_KEY".to_owned(),
            var("ARIAD_TEST_S3_SECRET_KEY", "ariadshift-test-secret"),
        ),
        ("AWS_REGION".to_owned(), "us-east-1".to_owned()),
        ("AWS_ALLOW_HTTP".to_owned(), "true".to_owned()),
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn pending_s3_uploads_are_size_checked_without_completion_and_after_replay() {
    let Some(env) =
        s3_env("pending_s3_uploads_are_size_checked_without_completion_and_after_replay")
    else {
        return;
    };
    ariad_server::install_crypto_provider();
    let config = StorageConfig::from_env(&Env::from_pairs(env.clone())).unwrap();
    let storage = Storage::from_config(&config, None, None).unwrap();
    let Some(h) = Harness::start(
        "pending_s3_uploads_are_size_checked_without_completion_and_after_replay",
        Options {
            env,
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    let bytes = b"# Actual uploaded body";
    let mut uploads = Vec::new();
    for declared in [1, bytes.len() as u64, 4096] {
        let response = h
            .post(
                &Auth::Anonymous,
                "/v1/uploads",
                &json!({"filename":"pending.md", "size": declared, "sha256": sha256_hex(bytes)}),
            )
            .await;
        assert_eq!(response.status(), 201);
        let upload: Value = response.json().await.unwrap();
        assert!(h.put_presigned(&upload, bytes).await.status().is_success());
        uploads.push(upload);
    }
    h.sql_exec("UPDATE uploads SET created_at = now() - interval '3 minutes'")
        .await;
    ariad_server::sweeper::sweep_locked(&h.db.pool, &storage)
        .await
        .unwrap()
        .unwrap();
    for (index, upload) in uploads.iter().enumerate() {
        let (status, filename, key): (String, String, String) =
            sqlx::query_as("SELECT status, filename, storage_key FROM uploads WHERE id = $1::uuid")
                .bind(upload["id"].as_str().unwrap())
                .fetch_one(&h.db.pool)
                .await
                .unwrap();
        if index == 1 {
            assert_eq!(status, "pending");
            assert_eq!(storage.size(&key).await.unwrap(), Some(bytes.len() as u64));
        } else {
            assert_eq!(status, "deleted");
            assert!(filename.is_empty());
            assert_eq!(storage.size(&key).await.unwrap(), None);
            // S3 signatures cannot be revoked individually before they expire.
            assert!(h.put_presigned(upload, bytes).await.status().is_success());
            ariad_server::sweeper::sweep_locked(&h.db.pool, &storage)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(storage.size(&key).await.unwrap(), None);
        }
    }
    h.sql_exec("UPDATE uploads SET expires_at = now() - interval '1 second'")
        .await;
    ariad_server::sweeper::sweep_locked(&h.db.pool, &storage)
        .await
        .unwrap()
        .unwrap();
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn presigned_requests_enforce_size_and_hash_on_s3() {
    let Some(env) = s3_env("presigned_requests_enforce_size_and_hash_on_s3") else {
        return;
    };
    ariad_server::install_crypto_provider();
    let config = StorageConfig::from_env(&Env::from_pairs(env)).expect("storage config");
    let storage = Storage::from_config(&config, None, None).expect("storage");
    let http = ariad_server::http_client();
    let body = b"# S3\n\nPresigned upload body.\n".to_vec();
    let sha = sha256_hex(&body);
    let key = storage.key(
        Uuid::new_v4(),
        Uuid::new_v4(),
        ariad_server::storage::Direction::In,
        &sha,
    );

    let presigned = storage
        .presign_put(&key, body.len() as u64, &sha)
        .await
        .expect("presign");
    assert_eq!(presigned.method, "PUT");
    assert!(presigned.headers.contains_key(CHECKSUM_HEADER));
    let put = |bytes: Vec<u8>| {
        let mut request = http.put(presigned.url.clone()).body(bytes);
        for (name, value) in &presigned.headers {
            if name != "content-length" {
                request = request.header(name.as_str(), value.as_str());
            }
        }
        request.send()
    };

    // A body that does not match the signed checksum is not stored as-is:
    // either the store refuses it, or `complete` finds the mismatch.
    let mut longer = body.clone();
    longer.push(b'!');
    let refused = put(longer).await.expect("PUT");
    if refused.status().is_success() {
        let digested = storage.digest(&key, 1 << 20).await.expect("digest");
        assert_ne!(digested.sha256, sha);
        storage.delete(&key).await.expect("delete");
    } else {
        assert!(
            refused.status().is_client_error(),
            "{}",
            text(refused).await
        );
        assert_eq!(storage.size(&key).await.expect("size"), None);
    }

    let stored = put(body.clone()).await.expect("PUT");
    assert!(stored.status().is_success(), "{}", text(stored).await);
    assert_eq!(
        storage.size(&key).await.expect("size"),
        Some(body.len() as u64)
    );
    let digested = storage
        .digest(&key, body.len() as u64)
        .await
        .expect("digest");
    assert_eq!(digested.sha256, sha);

    let (url, _) = storage.presign_get(&key).await.expect("presign get");
    let downloaded = http.get(url).send().await.expect("GET");
    assert!(downloaded.status().is_success());
    assert_eq!(downloaded.bytes().await.expect("bytes").to_vec(), body);

    storage.delete(&key).await.expect("delete");
    assert_eq!(storage.size(&key).await.expect("size"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_job_runs_end_to_end() {
    let Some(env) = s3_env("s3_job_runs_end_to_end") else {
        return;
    };
    let Some(mut h) = Harness::start(
        "s3_job_runs_end_to_end",
        Options {
            env,
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    h.start_worker();
    let auth = Auth::Anonymous;
    let body = b"# On S3\n\nConverted through object storage.\n";
    let upload = h.upload(&auth, "s3.md", body).await;
    let job: Value = h
        .create_job(&auth, upload["id"].as_str().expect("id"), "html")
        .await
        .json()
        .await
        .expect("job");
    let id = job["id"].as_str().expect("id").to_owned();
    let job = h.wait_final(&auth, &id, Duration::from_secs(60)).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    let outputs: Value = h
        .get(&auth, &format!("/v1/jobs/{id}/outputs"))
        .await
        .json()
        .await
        .expect("outputs");
    let url = outputs["outputs"][0]["url"].as_str().expect("url");
    assert!(url.contains("X-Amz-Signature"), "an S3 presigned URL");
    let html = h
        .http
        .get(url)
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("text");
    assert!(html.contains("On S3"), "{html}");
    assert_eq!(
        h.delete(&auth, &format!("/v1/jobs/{id}")).await.status(),
        204
    );
    let gone = h.http.get(url).send().await.expect("GET");
    assert!(!gone.status().is_success(), "deleted outputs are gone");
    h.stop().await;
}
