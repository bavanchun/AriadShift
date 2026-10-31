//! Global admission is atomic across API replicas and shared by REST and MCP.
mod common;
use common::{Auth, Harness, Options, TestIssuer};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Barrier;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_api_pools_cannot_overbook_or_leave_rejected_residue() {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        "global_admission",
        Options {
            restricted_logins: true,
            env: vec![
                ("ARIAD_GLOBAL_ACTIVE_JOBS".into(), "2".into()),
                ("ARIAD_AUTH_URL".into(), issuer.origin.clone()),
            ],
            ..Default::default()
        },
    )
    .await
    else {
        issuer.stop();
        return;
    };
    let login = format!("scheduler_api_{}", uuid::Uuid::new_v4().simple());
    let password = uuid::Uuid::new_v4().simple().to_string();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE ROLE \"{login}\" LOGIN PASSWORD '{password}' IN ROLE ariad_api"
    )))
    .execute(&h.db.pool)
    .await
    .unwrap();
    let mut second_db_url = url::Url::parse(&h.db.url).unwrap();
    second_db_url.set_username(&login).unwrap();
    second_db_url.set_password(Some(&password)).unwrap();
    let second_pool = ariad_server::db::connect(second_db_url.as_str(), 3)
        .await
        .unwrap();
    let elevated: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolcreaterole OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_one(&second_pool)
    .await
    .unwrap();
    assert!(!elevated);
    let storage = url::Url::from_directory_path(h.objects.path()).unwrap();
    let config =
        ariad_server::config::ApiConfig::from_env(&ariad_server::config::Env::from_pairs([
            ("ARIAD_SECRET", common::SECRET.to_string()),
            ("ARIAD_STORAGE_URL", storage.to_string()),
            ("ARIAD_PUBLIC_URL", h.base.to_string()),
            ("ARIAD_GLOBAL_ACTIVE_JOBS", "2".into()),
            ("ARIAD_AUTH_URL", issuer.origin.clone()),
        ]))
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_url = format!("http://{}/v1/jobs", listener.local_addr().unwrap());
    let stop = tokio_util::sync::CancellationToken::new();
    let token = stop.clone();
    let pool = second_pool.clone();
    let api = tokio::spawn(async move {
        ariad_server::app::serve_api(listener, config, pool, token)
            .await
            .unwrap();
    });
    let mut requests = Vec::new();
    let barrier = Arc::new(Barrier::new(7));
    for i in 0..6 {
        let auth = Auth::Bearer(issuer.token(&format!("subject-{i}")));
        let upload = h.upload(&auth, "test.md", b"# test").await;
        let request = auth
            .apply(h.http.post(if i % 2 == 0 {
                h.url("/v1/jobs").to_string()
            } else {
                second_url.clone()
            }))
            .json(&json!({"upload_id": upload["id"], "to": "html"}));
        let barrier = barrier.clone();
        requests.push(tokio::spawn(async move {
            barrier.wait().await;
            request.send().await.unwrap()
        }));
    }
    barrier.wait().await;
    let mut successes = 0;
    for request in requests {
        let response = request.await.unwrap();
        if response.status() == 202 {
            successes += 1;
        } else {
            assert_eq!(response.status(), 429);
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["error"]["code"], "quota_exceeded");
        }
    }
    assert_eq!(successes, 2, "only two fleet slots exist");
    for sql in [
        "SELECT count(*) FROM jobs",
        "SELECT count(*) FROM pgmq.q_jobs_core",
        "SELECT count(*) FROM usage_events WHERE kind='job_submitted'",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&h.db.pool)
                .await
                .unwrap(),
            2,
            "{sql}"
        );
    }
    stop.cancel();
    api.await.unwrap();
    second_pool.close().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP ROLE \"{login}\"")))
        .execute(&h.db.pool)
        .await
        .unwrap();
    h.stop().await;
    issuer.stop();
}

#[tokio::test]
async fn terminal_statuses_free_capacity_and_failed_transactions_leave_no_residue() {
    let Some(h) = Harness::start(
        "global_terminal_rollback",
        Options {
            env: vec![("ARIAD_GLOBAL_ACTIVE_JOBS".into(), "1".into())],
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    let upload = h.upload(&Auth::Anonymous, "test.md", b"# test").await;
    let upload = upload["id"].as_str().unwrap();
    for status in ["succeeded", "failed", "canceled", "expired"] {
        let response = h.create_job(&Auth::Anonymous, upload, "html").await;
        assert_eq!(response.status(), 202);
        let job: Value = response.json().await.unwrap();
        assert_eq!(
            h.create_job(&Auth::Anonymous, upload, "html")
                .await
                .status(),
            429
        );
        sqlx::query("UPDATE jobs SET status=$1, finished_at=now() WHERE id=$2::uuid")
            .bind(status)
            .bind(job["id"].as_str().unwrap())
            .execute(&h.db.pool)
            .await
            .unwrap();
    }
    sqlx::raw_sql("CREATE FUNCTION reject_test_message() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test send failure'; END $$; CREATE TRIGGER reject_test_message BEFORE INSERT ON pgmq.q_jobs_core FOR EACH ROW EXECUTE FUNCTION reject_test_message();")
        .execute(&h.db.pool).await.unwrap();
    assert_eq!(
        h.create_job(&Auth::Anonymous, upload, "html")
            .await
            .status(),
        500
    );
    for sql in [
        "SELECT count(*) FROM jobs",
        "SELECT count(*) FROM pgmq.q_jobs_core",
        "SELECT count(*) FROM usage_events WHERE kind='job_submitted'",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&h.db.pool)
                .await
                .unwrap(),
            4,
            "{sql}"
        );
    }
    sqlx::query("DROP TRIGGER reject_test_message ON pgmq.q_jobs_core")
        .execute(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(
        h.create_job(&Auth::Anonymous, upload, "html")
            .await
            .status(),
        202
    );
    h.stop().await;
}

#[tokio::test]
async fn mcp_and_rest_share_the_configured_global_ceiling_under_runtime_roles() {
    let Some(h) = Harness::start(
        "global_mcp",
        Options {
            restricted_logins: true,
            env: vec![("ARIAD_GLOBAL_ACTIVE_JOBS".into(), "1".into())],
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    let key = common::insert_api_key(&h.db.pool, "mcp-admission").await;
    let auth = Auth::ApiKey(key);
    let upload = h.upload(&auth, "test.md", b"# test").await;
    let first = h
        .create_job(&auth, upload["id"].as_str().unwrap(), "html")
        .await;
    assert_eq!(first.status(), 202);
    let job: Value = first.json().await.unwrap();
    let call = || {
        auth.apply(h.http.post(h.url("/mcp")))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/call").header("mcp-name", "convert")
        .json(&json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params": {
            "name":"convert", "arguments":{"upload_id":upload["id"], "to":"html"},
            "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28", "io.modelcontextprotocol/clientInfo":{"name":"admission-test","version":"1"}, "io.modelcontextprotocol/clientCapabilities":{}}
        }}))
    };
    let response: Value = call().send().await.unwrap().json().await.unwrap();
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert!(
        response.to_string().contains("quota_exceeded"),
        "{response}"
    );
    ariad_server::db::cancel_job(
        &mut h.db.pool.acquire().await.unwrap(),
        uuid::Uuid::parse_str(job["id"].as_str().unwrap()).unwrap(),
    )
    .await
    .unwrap();
    let response: Value = call().send().await.unwrap().json().await.unwrap();
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        h.create_job(&auth, upload["id"].as_str().unwrap(), "html")
            .await
            .status(),
        429
    );
    h.stop().await;
}

#[test]
fn global_ceiling_must_be_positive_and_finite() {
    for value in ["0", "-1", "not-a-number", "4294967296"] {
        let env = ariad_server::config::Env::from_pairs([
            ("ARIAD_SECRET", common::SECRET.as_str()),
            ("ARIAD_STORAGE_URL", "s3://test"),
            ("ARIAD_GLOBAL_ACTIVE_JOBS", value),
        ]);
        let error = ariad_server::config::ApiConfig::from_env(&env).expect_err(value);
        assert!(error.to_string().contains("ARIAD_GLOBAL_ACTIVE_JOBS"));
    }
}
