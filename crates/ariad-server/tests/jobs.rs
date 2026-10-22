//! The full job path: upload, job, events, outputs, deletion.

mod common;

use std::time::Duration;

use common::{Auth, Harness, Options, read_sse, sha256_hex, text};
use serde_json::{Value, json};

const MARKDOWN: &[u8] = b"# Quarterly notes\n\nThe *first* paragraph.\n\n- one\n- two\n";

async fn run_job(h: &Harness, auth: &Auth, to: &str) -> (Value, Vec<(String, Value)>) {
    let upload = h.upload(auth, "notes.md", MARKDOWN).await;
    assert_eq!(upload["status"], "completed");
    assert_eq!(upload["format"], "markdown");
    let response = h
        .create_job(auth, upload["id"].as_str().expect("id"), to)
        .await;
    assert_eq!(response.status(), 202, "{}", text(response).await);
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .expect("location");
    let job: Value = response.json().await.expect("job");
    let id = job["id"].as_str().expect("job id").to_owned();
    assert_eq!(location, format!("/v1/jobs/{id}"));
    assert_eq!(job["status"], "queued");
    assert_eq!(job["pack"], "core");
    assert!(!job["route"].as_array().expect("route").is_empty());

    let stream = auth
        .apply(h.http.get(h.url(&format!("/v1/jobs/{id}/events"))))
        .send()
        .await
        .expect("events");
    assert_eq!(stream.status(), 200);
    assert!(
        stream
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"))
    );
    let events = read_sse(stream, Duration::from_secs(120)).await;
    let job = h.wait_final(auth, &id, Duration::from_secs(30)).await;
    (job, events)
}

#[tokio::test(flavor = "multi_thread")]
async fn markdown_job_runs_end_to_end() {
    let Some(mut h) = Harness::start("markdown_job_runs_end_to_end", Options::default()).await
    else {
        return;
    };
    h.start_worker();
    let auth = Auth::Anonymous;
    let (job, events) = run_job(&h, &auth, "html").await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["attempts"], 1);
    assert!(job["error"].is_null());
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds.first(), Some(&"queued"), "{kinds:?}");
    assert!(kinds.contains(&"started"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"succeeded"), "{kinds:?}");
    let id = job["id"].as_str().expect("id");

    let outputs: Value = h
        .get(&auth, &format!("/v1/jobs/{id}/outputs"))
        .await
        .json()
        .await
        .expect("outputs");
    let output = &outputs["outputs"][0];
    assert_eq!(output["filename"], "notes.html");
    assert_eq!(output["format"], "html");
    assert_eq!(output["media_type"], "text/html");
    let body = h
        .http
        .get(output["url"].as_str().expect("url"))
        .send()
        .await
        .expect("download")
        .bytes()
        .await
        .expect("bytes");
    assert_eq!(sha256_hex(&body), output["sha256"].as_str().expect("sha"));
    assert_eq!(body.len() as u64, output["size"].as_u64().expect("size"));
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Quarterly notes"), "{html}");

    // Events replay after a cursor and stop at the final event.
    let last_started = events
        .iter()
        .find(|(k, _)| k == "started")
        .map(|(_, v)| v["id"].as_i64().expect("event id"))
        .expect("started");
    let replay = h
        .http
        .get(h.url(&format!("/v1/jobs/{id}/events")))
        .header("last-event-id", last_started.to_string())
        .send()
        .await
        .expect("replay");
    let replayed = read_sse(replay, Duration::from_secs(10)).await;
    assert!(
        replayed
            .iter()
            .all(|(_, v)| v["id"].as_i64() > Some(last_started))
    );
    assert_eq!(replayed.last().map(|(k, _)| k.as_str()), Some("succeeded"));

    let usage: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT kind, input_bytes, output_bytes FROM usage_events WHERE job_id = $1::uuid ORDER BY id",
    )
    .bind(id)
    .fetch_all(&h.db.pool)
    .await
    .expect("usage");
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0].0, "job_submitted");
    assert_eq!(usage[0].1, MARKDOWN.len() as i64);
    assert_eq!(usage[1].0, "job_completed");
    assert_eq!(usage[1].2, body.len() as i64);

    let archived: i64 = sqlx::query_scalar("SELECT count(*) FROM pgmq.a_jobs_core")
        .fetch_one(&h.db.pool)
        .await
        .expect("archive");
    assert_eq!(archived, 1, "the handled message is archived");

    // Delete now: files go at once and the job expires.
    assert_eq!(h.stored_files().len(), 2, "input and output stored");
    let response = h.delete(&auth, &format!("/v1/jobs/{id}")).await;
    assert_eq!(response.status(), 204);
    assert!(h.stored_files().is_empty(), "{:?}", h.stored_files());
    let gone = h.get(&auth, &format!("/v1/jobs/{id}/outputs")).await;
    assert_eq!(gone.status(), 410);
    let job: Value = h
        .get(&auth, &format!("/v1/jobs/{id}"))
        .await
        .json()
        .await
        .expect("job");
    assert_eq!(job["status"], "expired");
    let upload_status: String =
        sqlx::query_scalar("SELECT status FROM uploads WHERE id = $1::uuid")
            .bind(job["upload_id"].as_str().expect("upload id"))
            .fetch_one(&h.db.pool)
            .await
            .expect("upload status");
    assert_eq!(upload_status, "deleted");
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn markdown_to_docx_job_produces_a_document() {
    if ariad_host::pandoc_bin::locate().is_err() {
        eprintln!("skipping markdown_to_docx_job_produces_a_document: Pandoc is not available");
        return;
    }
    let Some(mut h) = Harness::start(
        "markdown_to_docx_job_produces_a_document",
        Options::default(),
    )
    .await
    else {
        return;
    };
    h.start_worker();
    let auth = Auth::Anonymous;
    let (job, events) = run_job(&h, &auth, "docx").await;
    assert_eq!(job["status"], "succeeded", "{job} {events:?}");
    let engines: Vec<&str> = job["route"]
        .as_array()
        .expect("route")
        .iter()
        .map(|s| s["engine"].as_str().expect("engine"))
        .collect();
    assert!(engines.contains(&"pandoc"), "{engines:?}");
    let id = job["id"].as_str().expect("id");
    let outputs: Value = h
        .get(&auth, &format!("/v1/jobs/{id}/outputs"))
        .await
        .json()
        .await
        .expect("outputs");
    let output = &outputs["outputs"][0];
    assert_eq!(output["filename"], "notes.docx");
    assert_eq!(
        output["media_type"],
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    let body = h
        .http
        .get(output["url"].as_str().expect("url"))
        .send()
        .await
        .expect("download")
        .bytes()
        .await
        .expect("bytes");
    assert!(body.starts_with(b"PK\x03\x04"), "a DOCX is a ZIP package");
    assert_eq!(sha256_hex(&body), output["sha256"].as_str().expect("sha"));
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_queued_job_cancels_it_before_any_worker_runs() {
    let Some(mut h) = Harness::start(
        "deleting_a_queued_job_cancels_it_before_any_worker_runs",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    let upload = h.upload(&auth, "notes.md", MARKDOWN).await;
    let job: Value = h
        .create_job(&auth, upload["id"].as_str().expect("id"), "html")
        .await
        .json()
        .await
        .expect("job");
    let id = job["id"].as_str().expect("id").to_owned();
    assert_eq!(
        h.delete(&auth, &format!("/v1/jobs/{id}")).await.status(),
        204
    );
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM job_events WHERE job_id = $1::uuid ORDER BY id")
            .bind(&id)
            .fetch_all(&h.db.pool)
            .await
            .expect("events");
    assert_eq!(kinds, ["queued", "canceled", "expired"]);

    // A worker started afterwards drops the stale message without running it.
    h.start_worker();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM pgmq.q_jobs_core")
            .fetch_one(&h.db.pool)
            .await
            .expect("queue");
        if queued == 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "message not dropped"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let job: Value = h
        .get(&auth, &format!("/v1/jobs/{id}"))
        .await
        .json()
        .await
        .expect("job");
    assert_eq!(job["status"], "expired");
    assert_eq!(job["attempts"], 0);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mismatching_and_unsupported_inputs_are_rejected() {
    let Some(h) = Harness::start(
        "mismatching_and_unsupported_inputs_are_rejected",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let auth = Auth::Anonymous;

    // A body that does not match the declared hash is refused by the
    // presigned URL, and the upload cannot complete.
    let upload = h.create_upload(&auth, "notes.md", MARKDOWN).await;
    let mut tampered = MARKDOWN.to_vec();
    tampered[0] = b'!';
    let put = h.put_presigned(&upload, &tampered).await;
    assert_eq!(put.status(), 422, "{}", text(put).await);
    let id = upload["id"].as_str().expect("id");
    let complete = h
        .post(&auth, &format!("/v1/uploads/{id}/complete"), &json!({}))
        .await;
    assert_eq!(complete.status(), 409);

    // A job needs a completed upload.
    let job = h.create_job(&auth, id, "html").await;
    assert_eq!(job.status(), 409);

    // Routes the planner cannot find are refused with the reachable targets.
    let upload = h.upload(&auth, "notes.md", MARKDOWN).await;
    let response = h
        .create_job(&auth, upload["id"].as_str().expect("id"), "png")
        .await;
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.expect("error");
    assert_eq!(body["error"]["code"], "unsupported_route");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("reachable targets")),
        "{body}"
    );

    let unknown = h
        .create_job(&auth, "00000000-0000-4000-8000-000000000001", "html")
        .await;
    assert_eq!(unknown.status(), 404);
    let bad_target = h
        .create_job(&auth, upload["id"].as_str().expect("id"), "nope")
        .await;
    assert_eq!(bad_target.status(), 422);
    let malformed = h.post(&auth, "/v1/jobs", &json!({ "to": "html" })).await;
    assert_eq!(malformed.status(), 422);
    let body: Value = malformed.json().await.expect("error");
    assert_eq!(body["error"]["code"], "invalid_request");
    assert_eq!(h.get(&auth, "/v1/jobs/not-a-uuid").await.status(), 404);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn plan_and_capabilities_describe_the_deployment() {
    let Some(h) = Harness::start(
        "plan_and_capabilities_describe_the_deployment",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    let response = h
        .post(&auth, "/v1/plan", &json!({ "from": "md", "to": "docx" }))
        .await;
    assert_eq!(response.status(), 200, "{}", text(response).await);
    let plan: Value = response.json().await.expect("plan");
    assert_eq!(plan["from"], "markdown");
    assert_eq!(plan["to"], "docx");
    assert_eq!(plan["pack"], "core");
    assert_eq!(plan["profile"], "editable");
    assert!(!plan["route"].as_array().expect("route").is_empty());

    let private = h
        .post(
            &auth,
            "/v1/plan",
            &json!({ "from": "md", "to": "docx", "profile": "private" }),
        )
        .await;
    assert_eq!(
        private.status(),
        422,
        "the private profile never runs in the cloud"
    );
    let both = h
        .post(
            &auth,
            "/v1/plan",
            &json!({ "from": "md", "upload_id": "00000000-0000-4000-8000-000000000001", "to": "docx" }),
        )
        .await;
    assert_eq!(both.status(), 422);

    let caps = h.get(&auth, "/v1/capabilities").await;
    assert_eq!(caps.status(), 200);
    assert!(caps.headers().get("cache-control").is_some());
    let caps: Value = caps.json().await.expect("capabilities");
    assert_eq!(caps["packs"], json!(["core"]));
    assert!(
        caps["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .all(|e| e["pack"] == "core")
    );
    assert!(
        caps["formats"]
            .as_array()
            .expect("formats")
            .iter()
            .any(|f| f["id"] == "markdown")
    );

    assert_eq!(h.get(&auth, "/healthz").await.status(), 200);
    let ready = h.get(&auth, "/readyz").await;
    assert_eq!(ready.status(), 200, "{}", text(ready).await);
    let request_id = h.get(&auth, "/healthz").await;
    assert!(request_id.headers().get("x-request-id").is_some());
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn transient_failures_retry_until_the_attempt_limit() {
    let Some(mut h) = Harness::start(
        "transient_failures_retry_until_the_attempt_limit",
        Options::default(),
    )
    .await
    else {
        return;
    };
    // An engine program that cannot start fails every Pandoc step.
    h.start_worker_with(&[
        ("ARIAD_ENGINE_PROGRAM", "/nonexistent/ashift"),
        ("ARIAD_WORKER_MAX_ATTEMPTS", "2"),
    ]);
    let auth = Auth::Anonymous;
    let upload = h.upload(&auth, "notes.md", MARKDOWN).await;
    let job: Value = h
        .create_job(&auth, upload["id"].as_str().expect("id"), "docx")
        .await
        .json()
        .await
        .expect("job");
    let id = job["id"].as_str().expect("id").to_owned();
    let job = h.wait_final(&auth, &id, Duration::from_secs(60)).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["attempts"], 2);
    let code = job["error"]["code"].as_str().expect("code");
    assert!(["tool_missing", "failed"].contains(&code), "{job}");
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM job_events WHERE job_id = $1::uuid ORDER BY id")
            .bind(&id)
            .fetch_all(&h.db.pool)
            .await
            .expect("events");
    assert_eq!(
        kinds,
        ["queued", "started", "retrying", "started", "failed"]
    );
    assert!(
        h.get(&auth, &format!("/v1/jobs/{id}/outputs"))
            .await
            .status()
            == 409
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopping_worker_hands_its_job_back() {
    use std::sync::Arc;

    use ariad_server::{
        config::{Env, WorkerConfig},
        packs::Pack,
        queue::{JobQueue, PgmqQueue},
        storage::Storage,
        telemetry::Metrics,
        worker::Worker,
    };
    use tokio_util::sync::CancellationToken;

    let Some(h) = Harness::start("a_stopping_worker_hands_its_job_back", Options::default()).await
    else {
        return;
    };
    let auth = Auth::Anonymous;
    let upload = h.upload(&auth, "notes.md", MARKDOWN).await;
    let job: Value = h
        .create_job(&auth, upload["id"].as_str().expect("id"), "html")
        .await
        .json()
        .await
        .expect("job");
    let id = job["id"].as_str().expect("id").to_owned();

    let mut pairs = vec![
        ("DATABASE_URL".to_owned(), h.db.url.clone()),
        ("ARIAD_SANDBOX".to_owned(), "off".to_owned()),
        ("ARIAD_ENGINE_PROGRAM".to_owned(), "/unused".to_owned()),
    ];
    pairs.extend(h.storage_env.clone());
    let config =
        WorkerConfig::from_env(&Env::from_pairs(pairs), &["core".to_owned()]).expect("config");
    let queue: Arc<dyn JobQueue> = Arc::new(PgmqQueue::new(h.db.pool.clone()));
    let worker = Worker {
        db: h.db.pool.clone(),
        queue: Arc::clone(&queue),
        storage: Storage::from_config(&config.storage, None, None).expect("storage"),
        config,
        metrics: Metrics::new(),
    };
    let delivery = queue
        .read(Pack::Core, Duration::from_secs(60), Duration::from_secs(1))
        .await
        .expect("read")
        .expect("a message");
    let stopping = CancellationToken::new();
    stopping.cancel();
    worker.handle(delivery, stopping).await;

    let job: Value = h
        .get(&auth, &format!("/v1/jobs/{id}"))
        .await
        .json()
        .await
        .expect("job");
    assert_eq!(job["status"], "queued", "{job}");
    assert_eq!(job["attempts"], 0, "a hand-back does not use up an attempt");
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM job_events WHERE job_id = $1::uuid ORDER BY id")
            .bind(&id)
            .fetch_all(&h.db.pool)
            .await
            .expect("events");
    assert_eq!(kinds, ["queued", "started", "retrying"]);
    let again = queue
        .read(Pack::Core, Duration::from_secs(60), Duration::from_secs(1))
        .await
        .expect("read")
        .expect("the message is visible again at once");
    assert_eq!(again.read_count, 2);
    h.stop().await;
}
