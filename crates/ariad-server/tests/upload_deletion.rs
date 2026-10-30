//! `DELETE /v1/uploads/{id}`: uploads that never became jobs.

mod common;

use common::{Auth, Harness, Options, TestIssuer, text};
use serde_json::{Value, json};

const MARKDOWN: &[u8] = b"# Delete me\n\nBody.\n";

async fn error(response: reqwest::Response) -> (u16, String) {
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    (
        status,
        body["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    )
}

async fn row(h: &Harness, id: &str) -> (String, String) {
    sqlx::query_as("SELECT status, filename FROM uploads WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&h.db.pool)
        .await
        .expect("upload row")
}

#[tokio::test(flavor = "multi_thread")]
async fn pending_uploads_can_be_deleted_and_free_their_slot() {
    let Some(h) = Harness::start(
        "pending_uploads_can_be_deleted_and_free_their_slot",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    h.sql_exec("UPDATE plan_entitlements SET concurrent_jobs = 1 WHERE plan = 'anonymous'")
        .await;
    let pending = h.create_upload(&auth, "first.md", MARKDOWN).await;
    let id = pending["id"].as_str().expect("id").to_owned();
    let refused = h
        .post(
            &auth,
            "/v1/uploads",
            &json!({ "filename": "second.md", "size": MARKDOWN.len(), "sha256": common::sha256_hex(MARKDOWN) }),
        )
        .await;
    assert_eq!(error(refused).await, (429, "quota_exceeded".to_owned()));

    let deleted = h.delete(&auth, &format!("/v1/uploads/{id}")).await;
    assert_eq!(deleted.status(), 204, "{}", text(deleted).await);
    assert_eq!(row(&h, &id).await, ("deleted".to_owned(), String::new()));

    // The pending slot is free again, and deleting twice succeeds.
    h.create_upload(&auth, "second.md", MARKDOWN).await;
    assert_eq!(
        h.delete(&auth, &format!("/v1/uploads/{id}")).await.status(),
        204
    );

    // A late transfer cannot revive it.
    assert!(
        h.put_presigned(&pending, MARKDOWN)
            .await
            .status()
            .is_success()
    );
    let complete = h
        .post(&auth, &format!("/v1/uploads/{id}/complete"), &json!({}))
        .await;
    assert_eq!(error(complete).await, (409, "conflict".to_owned()));
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn completed_uploads_are_deleted_with_their_file() {
    let Some(h) = Harness::start(
        "completed_uploads_are_deleted_with_their_file",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    let upload = h.upload(&auth, "done.md", MARKDOWN).await;
    let id = upload["id"].as_str().expect("id").to_owned();
    assert_eq!(h.stored_files().len(), 1);

    let deleted = h.delete(&auth, &format!("/v1/uploads/{id}")).await;
    assert_eq!(deleted.status(), 204, "{}", text(deleted).await);
    assert!(h.stored_files().is_empty(), "the stored file is gone");
    assert_eq!(row(&h, &id).await, ("deleted".to_owned(), String::new()));

    let job = h.create_job(&auth, &id, "html").await;
    assert_eq!(error(job).await, (409, "conflict".to_owned()));
    let plan = h
        .post(&auth, "/v1/plan", &json!({ "upload_id": id, "to": "html" }))
        .await;
    assert_eq!(error(plan).await.0, 409);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_that_feed_a_job_are_deleted_with_the_job() {
    let Some(h) = Harness::start(
        "uploads_that_feed_a_job_are_deleted_with_the_job",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    let upload = h.upload(&auth, "used.md", MARKDOWN).await;
    let id = upload["id"].as_str().expect("id").to_owned();
    // No worker runs, so the job stays queued.
    let job: Value = h
        .create_job(&auth, &id, "html")
        .await
        .json()
        .await
        .expect("job");
    let job_id = job["id"].as_str().expect("job id");

    let refused = h.delete(&auth, &format!("/v1/uploads/{id}")).await;
    assert_eq!(error(refused).await, (409, "conflict".to_owned()));
    assert_eq!(row(&h, &id).await.0, "completed");
    assert_eq!(h.stored_files().len(), 1, "the input is kept");

    assert_eq!(
        h.delete(&auth, &format!("/v1/jobs/{job_id}"))
            .await
            .status(),
        204
    );
    // The job deletion removed the upload; deleting it again succeeds.
    assert_eq!(
        h.delete(&auth, &format!("/v1/uploads/{id}")).await.status(),
        204
    );
    assert!(h.stored_files().is_empty());
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_are_deleted_only_by_their_workspace() {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        "uploads_are_deleted_only_by_their_workspace",
        Options {
            env: issuer.env(),
            ..Options::default()
        },
    )
    .await
    else {
        issuer.stop();
        return;
    };
    let alice = Auth::Bearer(issuer.token("alice"));
    let bob = Auth::Bearer(issuer.token("bob"));
    let upload = h.upload(&alice, "alice.md", MARKDOWN).await;
    let id = upload["id"].as_str().expect("id").to_owned();

    for caller in [&bob, &Auth::Anonymous] {
        let response = h.delete(caller, &format!("/v1/uploads/{id}")).await;
        assert_eq!(error(response).await, (404, "not_found".to_owned()));
    }
    for path in [
        "/v1/uploads/not-a-uuid".to_owned(),
        format!("/v1/uploads/{}", uuid::Uuid::new_v4()),
    ] {
        let response = h.delete(&alice, &path).await;
        assert_eq!(error(response).await, (404, "not_found".to_owned()));
    }
    assert_eq!(row(&h, &id).await.0, "completed");
    assert_eq!(h.stored_files().len(), 1);

    assert_eq!(
        h.delete(&alice, &format!("/v1/uploads/{id}"))
            .await
            .status(),
        204
    );
    assert!(h.stored_files().is_empty());
    h.stop().await;
    issuer.stop();
}
