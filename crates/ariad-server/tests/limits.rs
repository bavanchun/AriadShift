//! Entitlements, quotas, rate limits, the usage ledger and retention.

mod common;

use std::time::Duration;

use ariad_server::{
    db::{self, UsageEvent},
    sweeper,
};
use common::{Auth, Harness, Options, text};
use serde_json::{Value, json};

const MARKDOWN: &[u8] = b"# Limits\n\nBody.\n";

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

#[tokio::test(flavor = "multi_thread")]
async fn uploads_above_the_plan_limit_are_refused() {
    let Some(h) = Harness::start(
        "uploads_above_the_plan_limit_are_refused",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let too_big = 10 * 1024 * 1024 + 1;
    let response = h
        .post(
            &Auth::Anonymous,
            "/v1/uploads",
            &json!({ "filename": "big.md", "size": too_big, "sha256": "0".repeat(64) }),
        )
        .await;
    assert_eq!(error(response).await, (413, "limit_exceeded".to_owned()));
    let empty = h
        .post(
            &Auth::Anonymous,
            "/v1/uploads",
            &json!({ "filename": "empty.md", "size": 0, "sha256": "0".repeat(64) }),
        )
        .await;
    assert_eq!(empty.status(), 422);

    // The job keeps the plan's limits for the worker.
    let upload = h.upload(&Auth::Anonymous, "small.md", MARKDOWN).await;
    let job: Value = h
        .create_job(&Auth::Anonymous, upload["id"].as_str().expect("id"), "html")
        .await
        .json()
        .await
        .expect("job");
    let limits: Value = sqlx::query_scalar("SELECT limits FROM jobs WHERE id = $1::uuid")
        .bind(job["id"].as_str().expect("id"))
        .fetch_one(&h.db.pool)
        .await
        .expect("limits");
    assert_eq!(limits["max_input_bytes"], 10_485_760);
    assert_eq!(limits["timeout_s"], 120);
    assert_eq!(limits["max_memory_mb"], 1024);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn job_quotas_are_enforced_per_client() {
    let Some(h) = Harness::start("job_quotas_are_enforced_per_client", Options::default()).await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    let upload = h.upload(&auth, "q.md", MARKDOWN).await;
    let upload_id = upload["id"].as_str().expect("id").to_owned();

    // No worker runs, so jobs stay queued and count as concurrent.
    h.sql_exec("UPDATE plan_entitlements SET concurrent_jobs = 1 WHERE plan = 'anonymous'")
        .await;
    assert_eq!(h.create_job(&auth, &upload_id, "html").await.status(), 202);
    let response = h.create_job(&auth, &upload_id, "html").await;
    let (status, code) = error(response).await;
    assert_eq!((status, code.as_str()), (429, "quota_exceeded"));

    h.sql_exec(
        "UPDATE plan_entitlements SET concurrent_jobs = NULL, jobs_per_day = 2 \
         WHERE plan = 'anonymous'",
    )
    .await;
    assert_eq!(h.create_job(&auth, &upload_id, "html").await.status(), 202);
    assert_eq!(
        error(h.create_job(&auth, &upload_id, "html").await).await.0,
        429
    );

    let size = MARKDOWN.len() as i64;
    sqlx::query(
        "UPDATE plan_entitlements SET jobs_per_day = NULL, input_bytes_per_day = $1 \
         WHERE plan = 'anonymous'",
    )
    .bind(size * 3)
    .execute(&h.db.pool)
    .await
    .expect("bytes quota");
    assert_eq!(h.create_job(&auth, &upload_id, "html").await.status(), 202);
    let response = h.create_job(&auth, &upload_id, "html").await;
    assert_eq!(error(response).await, (429, "quota_exceeded".to_owned()));

    let submitted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM usage_events WHERE kind = 'job_submitted'")
            .fetch_one(&h.db.pool)
            .await
            .expect("usage");
    assert_eq!(submitted, 3, "refused jobs are not metered");
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_are_rate_limited_per_client() {
    let Some(h) = Harness::start("requests_are_rate_limited_per_client", Options::default()).await
    else {
        return;
    };
    h.sql_exec("UPDATE plan_entitlements SET requests_per_minute = 3 WHERE plan = 'anonymous'")
        .await;
    let body = json!({ "from": "md", "to": "html" });
    // Stay inside one minute window, so the count cannot reset midway.
    let now = chrono::Utc::now();
    let into_minute = now.timestamp() % 60;
    if into_minute > 50 {
        tokio::time::sleep(Duration::from_secs((61 - into_minute) as u64)).await;
    }
    for _ in 0..3 {
        let response = h.post(&Auth::Anonymous, "/v1/plan", &body).await;
        assert_eq!(response.status(), 200, "{}", text(response).await);
    }
    let limited = h.post(&Auth::Anonymous, "/v1/plan", &body).await;
    let retry_after: u64 = limited
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .expect("retry-after");
    assert!((1..=60).contains(&retry_after));
    assert_eq!(error(limited).await, (429, "rate_limited".to_owned()));
    // Health and capabilities stay reachable.
    assert_eq!(h.get(&Auth::Anonymous, "/healthz").await.status(), 200);
    assert_eq!(
        h.get(&Auth::Anonymous, "/v1/capabilities").await.status(),
        200
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_ledger_is_append_only_and_idempotent() {
    let Some(h) = Harness::start(
        "usage_ledger_is_append_only_and_idempotent",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let event = UsageEvent {
        event_key: "job:test:completed".to_owned(),
        workspace_id: db::ANONYMOUS_WORKSPACE,
        subject: "ip:test",
        job_id: None,
        kind: "job_completed",
        input_bytes: 10,
        output_bytes: 20,
        pages: 1,
        engine_ms: 5,
    };
    let mut conn = h.db.pool.acquire().await.expect("connection");
    assert!(
        db::record_usage(&mut conn, &event)
            .await
            .expect("first write")
    );
    assert!(
        !db::record_usage(&mut conn, &event)
            .await
            .expect("repeat write")
    );
    drop(conn);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM usage_events")
        .fetch_one(&h.db.pool)
        .await
        .expect("count");
    assert_eq!(rows, 1);
    for sql in [
        "UPDATE usage_events SET input_bytes = 0",
        "DELETE FROM usage_events",
        "TRUNCATE usage_events",
    ] {
        let err = sqlx::query(sql).execute(&h.db.pool).await.expect_err(sql);
        assert!(err.to_string().contains("append-only"), "{err}");
    }
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_deletes_files_and_expires_jobs() {
    let Some(mut h) = Harness::start(
        "retention_deletes_files_and_expires_jobs",
        Options::default(),
    )
    .await
    else {
        return;
    };
    h.start_worker();
    let auth = Auth::Anonymous;
    let upload = h.upload(&auth, "ttl.md", MARKDOWN).await;
    let upload_id = upload["id"].as_str().expect("id").to_owned();
    let job: Value = h
        .create_job(&auth, &upload_id, "html")
        .await
        .json()
        .await
        .expect("job");
    let job_id = job["id"].as_str().expect("id").to_owned();
    let job = h.wait_final(&auth, &job_id, Duration::from_secs(60)).await;
    assert_eq!(job["status"], "succeeded");
    let expires_at =
        chrono::DateTime::parse_from_rfc3339(job["expires_at"].as_str().expect("expiry"))
            .expect("timestamp");
    let created_at =
        chrono::DateTime::parse_from_rfc3339(job["created_at"].as_str().expect("created"))
            .expect("timestamp");
    assert_eq!(
        (expires_at - created_at).num_seconds(),
        3600,
        "one hour by default"
    );

    // A pending upload that never completed also expires.
    let abandoned = h.create_upload(&auth, "abandoned.md", MARKDOWN).await;
    assert_eq!(h.stored_files().len(), 2, "input and output");

    // Nothing is due yet.
    let report = sweeper::sweep_once(&h.db.pool, &storage(&h))
        .await
        .expect("sweep");
    assert_eq!(report, sweeper::SweepReport::default());

    h.sql_exec("UPDATE jobs SET expires_at = now() - interval '1 second'")
        .await;
    h.sql_exec("UPDATE uploads SET expires_at = now() - interval '1 second'")
        .await;
    let report = sweeper::sweep_locked(&h.db.pool, &storage(&h))
        .await
        .expect("sweep")
        .expect("lock acquired");
    assert_eq!(report.jobs, 1);
    assert_eq!(report.uploads, 2);
    assert!(h.stored_files().is_empty(), "{:?}", h.stored_files());

    let job: Value = h
        .get(&auth, &format!("/v1/jobs/{job_id}"))
        .await
        .json()
        .await
        .expect("job");
    assert_eq!(job["status"], "expired");
    assert_eq!(
        h.get(&auth, &format!("/v1/jobs/{job_id}/outputs"))
            .await
            .status(),
        410
    );
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM uploads ORDER BY created_at")
            .fetch_all(&h.db.pool)
            .await
            .expect("uploads");
    assert_eq!(statuses, ["expired", "expired"]);
    let abandoned_id = abandoned["id"].as_str().expect("id");
    let complete = h
        .post(
            &auth,
            &format!("/v1/uploads/{abandoned_id}/complete"),
            &json!({}),
        )
        .await;
    assert_eq!(complete.status(), 409);
    let last_event: String = sqlx::query_scalar(
        "SELECT kind FROM job_events WHERE job_id = $1::uuid ORDER BY id DESC LIMIT 1",
    )
    .bind(&job_id)
    .fetch_one(&h.db.pool)
    .await
    .expect("event");
    assert_eq!(last_event, "expired");

    // A second pass finds nothing.
    let report = sweeper::sweep_once(&h.db.pool, &storage(&h))
        .await
        .expect("sweep");
    assert_eq!(report, sweeper::SweepReport::default());
    h.stop().await;
}

fn storage(h: &Harness) -> ariad_server::storage::Storage {
    ariad_server::storage::Storage::from_config(
        &ariad_server::config::StorageConfig::Local {
            root: h.objects.path().to_path_buf(),
        },
        None,
        None,
    )
    .expect("storage")
}

#[tokio::test(flavor = "multi_thread")]
async fn abandoned_uploads_reserve_slots_and_daily_bytes() {
    let Some(h) = Harness::start(
        "abandoned_uploads_reserve_slots_and_daily_bytes",
        Options::default(),
    )
    .await
    else {
        return;
    };
    h.sql_exec("UPDATE plan_entitlements SET concurrent_jobs = 1, jobs_per_day = 2, input_bytes_per_day = 20 WHERE plan = 'anonymous'").await;
    let body = json!({"filename":"pending.md", "size":10, "sha256":"0".repeat(64)});
    let (a, b) = tokio::join!(
        h.post(&Auth::Anonymous, "/v1/uploads", &body),
        h.post(&Auth::Anonymous, "/v1/uploads", &body)
    );
    let mut statuses = [a.status().as_u16(), b.status().as_u16()];
    statuses.sort_unstable();
    assert_eq!(statuses, [201, 429]);
    let ttl: f64 =
        sqlx::query_scalar("SELECT extract(epoch FROM expires_at-created_at)::float8 FROM uploads")
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
    assert_eq!(ttl, 1200.0);
    h.sql_exec("UPDATE uploads SET status = 'deleted'").await;
    assert_eq!(
        h.post(&Auth::Anonymous, "/v1/uploads", &body)
            .await
            .status(),
        201
    );
    h.sql_exec("UPDATE uploads SET status = 'deleted'").await;
    assert_eq!(
        error(h.post(&Auth::Anonymous, "/v1/uploads", &body).await).await,
        (429, "quota_exceeded".to_owned())
    );
    let usage: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(usage, 0, "upload quota applies without submitting any job");
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_pending_uploads_are_deleted_without_completion() {
    let Some(h) = Harness::start(
        "expired_pending_uploads_are_deleted_without_completion",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let upload = h
        .create_upload(&Auth::Anonymous, "pending.md", MARKDOWN)
        .await;
    let put = &upload["upload"];
    let response = h
        .http
        .put(put["url"].as_str().unwrap())
        .body(MARKDOWN.to_vec())
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    h.sql_exec("UPDATE uploads SET expires_at = now() - interval '1 second'")
        .await;
    let id = upload["id"].as_str().unwrap();
    assert_eq!(
        h.post(
            &Auth::Anonymous,
            &format!("/v1/uploads/{id}/complete"),
            &json!({})
        )
        .await
        .status(),
        409
    );
    let report = sweeper::sweep_once(&h.db.pool, &storage(&h)).await.unwrap();
    assert_eq!(report.uploads, 1);
    assert!(h.stored_files().is_empty());
    h.stop().await;
}
