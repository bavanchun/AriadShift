//! Rejected credentials cannot lock out callers who can authenticate.

mod common;

use common::{Auth, Harness, Options, TestIssuer, create_apikey_table, insert_api_key};
use serde_json::{Value, json};

async fn failure_rows(h: &Harness) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM rate_limit_counters WHERE bucket LIKE 'auth-failure:%'",
    )
    .fetch_one(&h.db.pool)
    .await
    .unwrap()
}

async fn leave_time_for_failure_window() {
    let into_minute = chrono::Utc::now().timestamp().rem_euclid(60);
    if into_minute > 40 {
        tokio::time::sleep(std::time::Duration::from_secs(
            u64::try_from(61 - into_minute).unwrap(),
        ))
        .await;
    }
}

fn rpc(h: &Harness, auth: &Auth) -> reqwest::RequestBuilder {
    auth.apply(h.http.post(h.url("/mcp")))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/list")
        .json(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{
                "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28", "io.modelcontextprotocol/clientInfo":{"name":"ariad-integration","version":"1"}, "io.modelcontextprotocol/clientCapabilities":{}}
            }}),
        )
}

#[tokio::test(flavor = "multi_thread")]
async fn bare_rest_requests_leave_the_failure_bucket_empty() {
    let issuer = TestIssuer::start().await;
    let mut env = issuer.env();
    env.push(("ARIAD_ANONYMOUS".into(), "false".into()));
    let Some(h) = Harness::start(
        "bare_rest",
        Options {
            env,
            ..Options::default()
        },
    )
    .await
    else {
        issuer.stop();
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let body = json!({"from":"md","to":"html"});
    for _ in 0..35 {
        assert_eq!(
            h.post(&Auth::Anonymous, "/v1/plan", &body).await.status(),
            401
        );
    }
    assert_eq!(failure_rows(&h).await, 0);
    for auth in [
        Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await),
        Auth::Bearer(issuer.token("alice")),
    ] {
        assert_eq!(h.post(&auth, "/v1/plan", &body).await.status(), 200);
    }
    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn bare_mcp_probes_keep_the_metadata_challenge_and_valid_credentials_work() {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        "bare_mcp",
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
    create_apikey_table(&h.db.pool).await;
    for _ in 0..35 {
        let response = rpc(&h, &Auth::Anonymous).send().await.unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(
            response.headers()["www-authenticate"],
            format!(
                "Bearer resource_metadata=\"{}\"",
                h.url("/.well-known/oauth-protected-resource/mcp")
            )
        );
    }
    assert_eq!(failure_rows(&h).await, 0);
    let audience = h.url("/mcp").to_string();
    for auth in [
        Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await),
        Auth::Bearer(issuer.token_with("alice", 900, &audience, &issuer.kid)),
    ] {
        let response = rpc(&h, &auth).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert!(body["result"]["tools"].is_array(), "{body}");
    }
    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_present_credentials_throttle_without_blocking_valid_keys_or_tokens() {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        "present_failures",
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
    create_apikey_table(&h.db.pool).await;
    let body = json!({"from":"md","to":"html"});
    leave_time_for_failure_window().await;
    for i in 0..ariad_server::api::AUTH_FAILURES_PER_MINUTE {
        let invalid = if i % 2 == 0 {
            Auth::ApiKey("g".repeat(64))
        } else {
            Auth::Bearer("invalid.token".into())
        };
        assert_eq!(h.post(&invalid, "/v1/plan", &body).await.status(), 401);
    }
    let response = h
        .post(&Auth::ApiKey("g".repeat(64)), "/v1/plan", &body)
        .await;
    assert_eq!(response.status(), 429);
    assert!(response.headers().contains_key("retry-after"));
    for auth in [
        Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await),
        Auth::Bearer(issuer.token("alice")),
    ] {
        assert_eq!(h.post(&auth, "/v1/plan", &body).await.status(), 200);
    }
    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_mcp_credentials_throttle_but_bare_probes_and_valid_tokens_still_work() {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        "mcp_present_failures",
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
    create_apikey_table(&h.db.pool).await;
    leave_time_for_failure_window().await;
    for _ in 0..ariad_server::api::AUTH_FAILURES_PER_MINUTE {
        assert_eq!(
            rpc(&h, &Auth::Bearer("invalid.token".into()))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    let rejected = rpc(&h, &Auth::Bearer("invalid.token".into()))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 429);
    assert!(rejected.headers().contains_key("retry-after"));
    let bare = rpc(&h, &Auth::Anonymous).send().await.unwrap();
    assert_eq!(bare.status(), 401);
    assert!(bare.headers().contains_key("www-authenticate"));
    let audience = h.url("/mcp").to_string();
    for auth in [
        Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await),
        Auth::Bearer(issuer.token_with("alice", 900, &audience, &issuer.kid)),
    ] {
        assert_eq!(rpc(&h, &auth).send().await.unwrap().status(), 200);
    }
    h.stop().await;
    issuer.stop();
}
