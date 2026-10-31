//! `GET /v1/limits`: plan limits, quotas and retention before an upload.

mod common;

use common::{Auth, Harness, Options, create_apikey_table, insert_api_key, text};
use serde_json::{Value, json};

const MARKDOWN: &[u8] = b"# Limits\n\nBody.\n";

async fn limits(h: &Harness, auth: &Auth) -> Value {
    let response = h.get(auth, "/v1/limits").await;
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("private, no-store")
    );
    assert_eq!(response.status(), 200, "limits: {}", text(response).await);
    response.json().await.expect("limits JSON")
}

#[tokio::test(flavor = "multi_thread")]
async fn anonymous_callers_see_the_anonymous_plan_and_their_usage() {
    let Some(h) = Harness::start(
        "anonymous_callers_see_the_anonymous_plan_and_their_usage",
        Options {
            keep_quotas: true,
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    let before = limits(&h, &auth).await;
    assert_eq!(before["plan"], "anonymous");
    assert_eq!(before["authenticated"], false);
    assert_eq!(before["retention_s"], 3600);
    assert_eq!(before["requests_per_minute"], 30);
    assert_eq!(
        before["limits"],
        json!({
            "max_input_bytes": 10_485_760,
            "max_pages": 50,
            "timeout_s": 120,
            "max_memory_mb": 1024,
        })
    );
    let quotas = &before["quotas"];
    assert_eq!(
        quotas["uploads_per_day"],
        json!({ "limit": 20, "used": 0, "remaining": 20 })
    );
    assert_eq!(
        quotas["jobs_per_day"],
        json!({ "limit": 20, "used": 0, "remaining": 20 })
    );
    assert_eq!(
        quotas["input_bytes_per_day"],
        json!({ "limit": 104_857_600, "used": 0, "remaining": 104_857_600 })
    );
    assert_eq!(
        quotas["pending_uploads"],
        json!({ "limit": 1, "used": 0, "remaining": 1 })
    );
    assert_eq!(
        quotas["concurrent_jobs"],
        json!({ "limit": 1, "used": 0, "remaining": 1 })
    );

    // No worker runs, so the job stays queued.
    let upload = h.upload(&auth, "limits.md", MARKDOWN).await;
    let job = h
        .create_job(&auth, upload["id"].as_str().expect("id"), "html")
        .await;
    assert_eq!(job.status(), 202, "{}", text(job).await);
    let pending = h.create_upload(&auth, "pending.md", MARKDOWN).await;
    assert_eq!(pending["status"], "pending");

    let after = limits(&h, &auth).await;
    let size = MARKDOWN.len() as u64;
    let quotas = &after["quotas"];
    assert_eq!(quotas["uploads_per_day"]["used"], 2);
    assert_eq!(quotas["uploads_per_day"]["remaining"], 18);
    assert_eq!(quotas["jobs_per_day"]["remaining"], 19);
    assert_eq!(quotas["input_bytes_per_day"]["used"], 2 * size);
    assert_eq!(
        quotas["input_bytes_per_day"]["remaining"],
        104_857_600 - 2 * size
    );
    assert_eq!(quotas["pending_uploads"]["remaining"], 0);
    assert_eq!(quotas["concurrent_jobs"]["remaining"], 0);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn account_callers_see_their_workspace_plan() {
    let Some(h) = Harness::start(
        "account_callers_see_their_workspace_plan",
        Options {
            keep_quotas: true,
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let key = Auth::ApiKey(insert_api_key(&h.db.pool, "limits-user").await);
    let body = limits(&h, &key).await;
    assert_eq!(body["plan"], "free");
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["limits"]["max_input_bytes"], 2_147_483_648_u64);
    assert_eq!(body["quotas"]["jobs_per_day"]["limit"], 200);
    assert_eq!(body["quotas"]["concurrent_jobs"]["limit"], 4);

    // Anonymous usage does not count against the account.
    h.upload(&Auth::Anonymous, "anonymous.md", MARKDOWN).await;
    let body = limits(&h, &key).await;
    assert_eq!(body["quotas"]["uploads_per_day"]["used"], 0);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unlimited_quotas_are_null() {
    let Some(h) = Harness::start("unlimited_quotas_are_null", Options::default()).await else {
        return;
    };
    // The harness lifts every quota and rate limit.
    let body = limits(&h, &Auth::Anonymous).await;
    assert_eq!(body["requests_per_minute"], Value::Null);
    for quota in [
        "uploads_per_day",
        "jobs_per_day",
        "input_bytes_per_day",
        "concurrent_jobs",
    ] {
        assert_eq!(body["quotas"][quota]["limit"], Value::Null, "{quota}");
        assert_eq!(body["quotas"][quota]["remaining"], Value::Null, "{quota}");
    }
    // Upload admission still caps pending uploads.
    assert_eq!(body["quotas"]["pending_uploads"]["limit"], 64);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_credentials_get_no_limits() {
    let Some(h) = Harness::start("rejected_credentials_get_no_limits", Options::default()).await
    else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let response = h.get(&Auth::ApiKey("k".repeat(64)), "/v1/limits").await;
    assert_eq!(response.status(), 401, "{}", text(response).await);
    h.stop().await;
}
