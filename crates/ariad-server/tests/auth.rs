//! Authentication: Better Auth JWTs and API keys, anonymous access, Turnstile.

mod common;

use common::{
    Auth, Harness, Options, TestIssuer, create_apikey_table, insert_api_key, start_turnstile, text,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};

const MARKDOWN: &[u8] = b"# Private\n\nWorkspace data.\n";

async fn error_code(response: reqwest::Response) -> (u16, String) {
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
async fn bearer_tokens_are_verified_against_the_jwks() {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        "bearer_tokens_are_verified_against_the_jwks",
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

    // A valid token creates the account's workspace on first use.
    let upload = h.upload(&alice, "alice.md", MARKDOWN).await;
    let job: Value = h
        .create_job(&alice, upload["id"].as_str().expect("id"), "html")
        .await
        .json()
        .await
        .expect("job");
    let job_id = job["id"].as_str().expect("id");
    let (workspace, plan): (uuid::Uuid, String) =
        sqlx::query_as("SELECT id, plan FROM workspaces WHERE owner_user_id = 'alice'")
            .fetch_one(&h.db.pool)
            .await
            .expect("workspace");
    assert_eq!(plan, "free");
    let key: String = sqlx::query_scalar("SELECT storage_key FROM uploads WHERE id = $1::uuid")
        .bind(upload["id"].as_str().expect("id"))
        .fetch_one(&h.db.pool)
        .await
        .expect("key");
    assert!(key.starts_with(&format!("t/{workspace}/")), "{key}");
    let limits: Value = sqlx::query_scalar("SELECT limits FROM jobs WHERE id = $1::uuid")
        .bind(job_id)
        .fetch_one(&h.db.pool)
        .await
        .expect("limits");
    assert_eq!(
        limits["max_input_bytes"], 2_147_483_648_u64,
        "free plan limits"
    );
    assert_eq!(limits["timeout_s"], 600);

    // Another account cannot see it, and neither can anonymous callers.
    assert_eq!(
        h.get(&bob, &format!("/v1/jobs/{job_id}")).await.status(),
        404
    );
    assert_eq!(
        h.get(&Auth::Anonymous, &format!("/v1/jobs/{job_id}"))
            .await
            .status(),
        404
    );
    assert_eq!(
        h.get(&alice, &format!("/v1/jobs/{job_id}")).await.status(),
        200
    );
    let bob_job = h
        .create_job(&bob, upload["id"].as_str().expect("id"), "html")
        .await;
    assert_eq!(bob_job.status(), 404, "uploads belong to their workspace");

    // Expired, foreign-audience, unknown-key and symmetric tokens fail.
    let expired = Auth::Bearer(issuer.token_with("alice", -120, &issuer.origin, &issuer.kid));
    assert_eq!(
        error_code(h.get(&expired, &format!("/v1/jobs/{job_id}")).await).await,
        (401, "unauthorized".to_owned())
    );
    let foreign =
        Auth::Bearer(issuer.token_with("alice", 900, "https://elsewhere.example", &issuer.kid));
    assert_eq!(
        h.get(&foreign, &format!("/v1/jobs/{job_id}"))
            .await
            .status(),
        401
    );
    let unknown_kid = Auth::Bearer(issuer.token_with("alice", 900, &issuer.origin, "rotated-away"));
    assert_eq!(
        h.get(&unknown_kid, &format!("/v1/jobs/{job_id}"))
            .await
            .status(),
        401
    );
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(issuer.kid.clone());
    let symmetric = jsonwebtoken::encode(
        &header,
        &json!({ "sub": "alice", "iss": issuer.origin, "aud": issuer.origin, "exp": 4_102_444_800_u64 }),
        &EncodingKey::from_secret(b"guessable"),
    )
    .expect("sign");
    assert_eq!(
        h.get(&Auth::Bearer(symmetric), &format!("/v1/jobs/{job_id}"))
            .await
            .status(),
        401
    );
    let garbage = Auth::Bearer("not.a.token".to_owned());
    assert_eq!(
        h.post(&garbage, "/v1/plan", &json!({ "from": "md", "to": "html" }))
            .await
            .status(),
        401
    );
    let basic = h
        .http
        .get(h.url(&format!("/v1/jobs/{job_id}")))
        .header("authorization", "Basic YWxpY2U6cHc=")
        .send()
        .await
        .expect("request");
    assert_eq!(basic.status(), 401);

    issuer.stop();
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_tokens_need_a_configured_issuer() {
    let issuer = TestIssuer::start().await;
    let Some(h) =
        Harness::start("bearer_tokens_need_a_configured_issuer", Options::default()).await
    else {
        issuer.stop();
        return;
    };
    let token = Auth::Bearer(issuer.token("alice"));
    assert_eq!(
        error_code(
            h.post(&token, "/v1/plan", &json!({ "from": "md", "to": "html" }))
                .await
        )
        .await,
        (401, "unauthorized".to_owned())
    );
    issuer.stop();
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn api_keys_are_checked_read_only() {
    let Some(h) = Harness::start("api_keys_are_checked_read_only", Options::default()).await else {
        return;
    };
    // A missing Better Auth schema is an unavailable verifier, not a bad key.
    let early = Auth::ApiKey("x".repeat(64));
    assert_eq!(
        h.get(&early, "/v1/jobs/00000000-0000-4000-8000-000000000001")
            .await
            .status(),
        503
    );

    create_apikey_table(&h.db.pool).await;
    let raw = insert_api_key(&h.db.pool, "carol").await;
    let carol = Auth::ApiKey(raw.clone());
    let upload = h.upload(&carol, "carol.md", MARKDOWN).await;
    assert_eq!(upload["status"], "completed");
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT w.owner_user_id FROM uploads u JOIN workspaces w ON w.id = u.workspace_id \
         WHERE u.id = $1::uuid",
    )
    .bind(upload["id"].as_str().expect("id"))
    .fetch_one(&h.db.pool)
    .await
    .expect("owner");
    assert_eq!(owner.as_deref(), Some("carol"));

    // Better Auth's counters are never written.
    let (count, last): (Option<i32>, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        r#"SELECT "requestCount", "lastRequest" FROM "apikey" WHERE "referenceId" = 'carol'"#,
    )
    .fetch_one(&h.db.pool)
    .await
    .expect("counters");
    assert_eq!(count, Some(0));
    assert!(last.is_none());

    let last = if raw.ends_with('x') { 'y' } else { 'x' };
    let wrong = Auth::ApiKey(format!("{}{last}", &raw[..63]));
    assert_eq!(
        error_code(h.get(&wrong, "/v1/capabilities").await).await.0,
        200,
        "public route"
    );
    assert_eq!(
        error_code(
            h.post(&wrong, "/v1/plan", &json!({ "from": "md", "to": "html" }))
                .await
        )
        .await,
        (401, "unauthorized".to_owned())
    );
    assert_eq!(
        h.post(
            &Auth::ApiKey("short".to_owned()),
            "/v1/plan",
            &json!({ "from": "md", "to": "html" })
        )
        .await
        .status(),
        401
    );

    let plan_request = json!({ "from": "md", "to": "html" });
    let check = |sql: &'static str| {
        let pool = h.db.pool.clone();
        async move {
            sqlx::query(sql).execute(&pool).await.expect(sql);
        }
    };
    check(r#"UPDATE "apikey" SET "enabled" = false WHERE "referenceId" = 'carol'"#).await;
    assert_eq!(
        h.post(&carol, "/v1/plan", &plan_request).await.status(),
        401
    );
    check(r#"UPDATE "apikey" SET "enabled" = NULL, "expiresAt" = now() - interval '1 minute' WHERE "referenceId" = 'carol'"#).await;
    assert_eq!(
        h.post(&carol, "/v1/plan", &plan_request).await.status(),
        401
    );
    check(
        r#"UPDATE "apikey" SET "expiresAt" = NULL, "remaining" = 0 WHERE "referenceId" = 'carol'"#,
    )
    .await;
    assert_eq!(
        error_code(h.post(&carol, "/v1/plan", &plan_request).await).await,
        (429, "quota_exceeded".to_owned())
    );
    check(r#"UPDATE "apikey" SET "remaining" = NULL, "requestCount" = 10, "lastRequest" = now() WHERE "referenceId" = 'carol'"#).await;
    let limited = h.post(&carol, "/v1/plan", &plan_request).await;
    assert!(limited.headers().get("retry-after").is_some());
    assert_eq!(error_code(limited).await, (429, "rate_limited".to_owned()));
    check(r#"UPDATE "apikey" SET "requestCount" = 0, "configId" = 'organization-keys' WHERE "referenceId" = 'carol'"#).await;
    assert_eq!(
        h.post(&carol, "/v1/plan", &plan_request).await.status(),
        401
    );
    check(r#"UPDATE "apikey" SET "configId" = 'default' WHERE "referenceId" = 'carol'"#).await;
    assert_eq!(
        h.post(&carol, "/v1/plan", &plan_request).await.status(),
        200
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_credentials_are_throttled_per_client() {
    let Some(h) = Harness::start(
        "rejected_credentials_are_throttled_per_client",
        Options::default(),
    )
    .await
    else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let body = json!({ "from": "md", "to": "html" });
    let guess = Auth::ApiKey("g".repeat(64));
    // Stay inside one minute window, so the count cannot reset midway.
    let into_minute = chrono::Utc::now().timestamp() % 60;
    if into_minute > 45 {
        tokio::time::sleep(std::time::Duration::from_secs((61 - into_minute) as u64)).await;
    }
    let limit = ariad_server::api::AUTH_FAILURES_PER_MINUTE;
    for _ in 0..limit {
        assert_eq!(h.post(&guess, "/v1/plan", &body).await.status(), 401);
    }
    let throttled = h.post(&guess, "/v1/plan", &body).await;
    assert!(throttled.headers().get("retry-after").is_some());
    assert_eq!(
        error_code(throttled).await,
        (429, "rate_limited".to_owned())
    );
    let valid = Auth::ApiKey(insert_api_key(&h.db.pool, "throttled-user").await);
    assert_eq!(h.post(&valid, "/v1/plan", &body).await.status(), 429);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn api_keys_can_be_disabled() {
    let Some(h) = Harness::start(
        "api_keys_can_be_disabled",
        Options {
            env: vec![("ARIAD_API_KEYS".to_owned(), "false".to_owned())],
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let raw = insert_api_key(&h.db.pool, "dave").await;
    let response = h
        .post(
            &Auth::ApiKey(raw),
            "/v1/plan",
            &json!({ "from": "md", "to": "html" }),
        )
        .await;
    assert_eq!(response.status(), 401);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn anonymous_access_can_be_turned_off() {
    let Some(h) = Harness::start(
        "anonymous_access_can_be_turned_off",
        Options {
            env: vec![("ARIAD_ANONYMOUS".to_owned(), "false".to_owned())],
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    let response = h
        .post(
            &Auth::Anonymous,
            "/v1/uploads",
            &json!({ "filename": "a.md", "size": 3, "sha256": "0".repeat(64) }),
        )
        .await;
    assert_eq!(error_code(response).await, (401, "unauthorized".to_owned()));
    assert_eq!(
        h.get(&Auth::Anonymous, "/v1/capabilities").await.status(),
        200
    );
    assert_eq!(h.get(&Auth::Anonymous, "/healthz").await.status(), 200);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn turnstile_guards_anonymous_uploads() {
    let (verify_url, stop_turnstile) = start_turnstile().await;
    let Some(h) = Harness::start(
        "turnstile_guards_anonymous_uploads",
        Options {
            env: vec![
                (
                    "ARIAD_TURNSTILE_SECRET".to_owned(),
                    "turnstile-secret".to_owned(),
                ),
                ("ARIAD_TURNSTILE_VERIFY_URL".to_owned(), verify_url),
            ],
            ..Options::default()
        },
    )
    .await
    else {
        stop_turnstile.cancel();
        return;
    };
    let body = json!({ "filename": "a.md", "size": 3, "sha256": "0".repeat(64) });
    let missing = h.post(&Auth::Anonymous, "/v1/uploads", &body).await;
    assert_eq!(
        error_code(missing).await,
        (403, "turnstile_required".to_owned())
    );
    let bad = h
        .http
        .post(h.url("/v1/uploads"))
        .header("cf-turnstile-response", "bad-token")
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(error_code(bad).await, (403, "turnstile_failed".to_owned()));
    let good = h
        .http
        .post(h.url("/v1/uploads"))
        .header("cf-turnstile-response", "good-token")
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(good.status(), 201, "{}", text(good).await);

    // Accounts are not challenged.
    create_apikey_table(&h.db.pool).await;
    let raw = insert_api_key(&h.db.pool, "erin").await;
    let keyed = h.post(&Auth::ApiKey(raw), "/v1/uploads", &body).await;
    assert_eq!(keyed.status(), 201);
    stop_turnstile.cancel();
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn api_key_lookup_outages_do_not_count_as_rejected_credentials() {
    let Some(h) = Harness::start(
        "api_key_lookup_outages_do_not_count_as_rejected_credentials",
        Options::default(),
    )
    .await
    else {
        return;
    };
    let key = Auth::ApiKey("a".repeat(64));
    for _ in 0..31 {
        assert_eq!(
            error_code(
                h.post(&key, "/v1/plan", &json!({"from":"md","to":"html"}))
                    .await
            )
            .await,
            (503, "unavailable".to_owned())
        );
    }
    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rate_limit_counters WHERE bucket LIKE 'auth-failure:%'",
    )
    .fetch_one(&h.db.pool)
    .await
    .unwrap();
    assert_eq!(failures, 0);
    h.stop().await;
}
