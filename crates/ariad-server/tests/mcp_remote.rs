//! Remote MCP uses real auth, admission, storage and conversion workers.

mod common;

use std::time::Duration;

use common::{Auth, Harness, Options, TestIssuer, create_apikey_table, insert_api_key};
use serde_json::{Value, json};

const VERSION: &str = "2026-07-28";

fn rpc(h: &Harness, auth: &Auth, method: &str, mut params: Value) -> reqwest::RequestBuilder {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": VERSION,
        "io.modelcontextprotocol/clientInfo": {"name": "ariad-integration", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let mut builder = auth
        .apply(h.http.post(h.base.join("mcp").unwrap()))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", VERSION)
        .header("mcp-method", method);
    if method == "tools/call" {
        builder = builder.header("mcp-name", params["name"].as_str().unwrap());
    }
    builder.json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
}

async fn call(h: &Harness, auth: &Auth, name: &str, arguments: Value) -> Value {
    let response = rpc(
        h,
        auth,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
    .send()
    .await
    .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(body.get("error").is_none(), "{body}");
    body["result"].clone()
}

async fn wait_for_success(h: &Harness, auth: &Auth, job_id: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "conversion timed out"
        );
        let result = call(h, auth, "job_status", json!({"job_id": job_id})).await;
        assert_ne!(result["isError"], true, "{result}");
        match result["structuredContent"]["job"]["status"]
            .as_str()
            .unwrap()
        {
            "succeeded" => return result,
            "failed" | "canceled" | "expired" => panic!("conversion did not succeed: {result}"),
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn oauth_metadata_challenge_and_audience_are_enforced() {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        "mcp_oauth",
        Options {
            env: vec![("ARIAD_AUTH_URL".to_owned(), issuer.origin.clone())],
            ..Options::default()
        },
    )
    .await
    else {
        issuer.stop();
        return;
    };
    let resource = h.base.join("mcp").unwrap().to_string();
    let metadata_url = h
        .base
        .join(".well-known/oauth-protected-resource/mcp")
        .unwrap();
    let metadata_response = h.http.get(metadata_url.clone()).send().await.unwrap();
    assert_eq!(metadata_response.status(), 200);
    let metadata: Value = metadata_response.json().await.unwrap();
    assert_eq!(metadata["resource"], resource);
    assert_eq!(metadata["authorization_servers"], json!([issuer.origin]));
    assert_eq!(metadata["bearer_methods_supported"], json!(["header"]));
    // The root alias serves the same document, and browser clients on any
    // origin may read it; the Host and Origin checks guard `/mcp` only.
    let root = h
        .http
        .get(h.base.join(".well-known/oauth-protected-resource").unwrap())
        .header("origin", "https://inspector.example.test")
        .send()
        .await
        .unwrap();
    assert_eq!(root.status(), 200);
    assert_eq!(root.headers()["access-control-allow-origin"], "*");
    assert_eq!(root.json::<Value>().await.unwrap(), metadata);
    let cross_origin = h
        .http
        .get(metadata_url.clone())
        .header("origin", "https://inspector.example.test")
        .send()
        .await
        .unwrap();
    assert_eq!(cross_origin.status(), 200);
    assert_eq!(cross_origin.headers()["access-control-allow-origin"], "*");

    for auth in [
        Auth::Anonymous,
        Auth::Bearer("invalid.token".to_owned()),
        Auth::Bearer(issuer.token("alice")),
        Auth::Bearer(issuer.token_with("alice", -120, &resource, &issuer.kid)),
    ] {
        let response = rpc(&h, &auth, "tools/list", json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert!(
            response.headers()["www-authenticate"]
                .to_str()
                .unwrap()
                .contains(metadata_url.as_str())
        );
    }
    let alice = Auth::Bearer(issuer.token_with("alice", 900, &resource, &issuer.kid));
    let empty_allowlist = rpc(&h, &alice, "tools/list", json!({}))
        .header("origin", h.base.origin().ascii_serialization())
        .send()
        .await
        .unwrap();
    assert_eq!(empty_allowlist.status(), 403);
    let response = rpc(&h, &alice, "tools/list", json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers().get("mcp-session-id").is_none());
    let body: Value = response.json().await.unwrap();
    let names: Vec<_> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["list_engines", "plan", "inspect", "convert", "job_status"]
    );
    let discovery = rpc(&h, &alice, "server/discover", json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(discovery.status(), 200);
    let discovery: Value = discovery.json().await.unwrap();
    assert!(discovery.get("error").is_none(), "{discovery}");

    let mut mismatched_headers = reqwest::header::HeaderMap::new();
    mismatched_headers.insert("mcp-method", "tools/call".parse().unwrap());
    let mismatch = rpc(&h, &alice, "tools/list", json!({}))
        .headers(mismatched_headers)
        .send()
        .await
        .unwrap();
    assert_eq!(mismatch.status(), 400);
    // Tokens bound to the MCP resource are not REST credentials.
    assert_eq!(
        h.get(&alice, "/v1/jobs/00000000-0000-0000-0000-000000000000")
            .await
            .status(),
        401
    );
    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn origins_hosts_methods_and_request_sizes_are_bounded() {
    let Some(h) = Harness::start(
        "mcp_transport_bounds",
        Options {
            env: vec![(
                "ARIAD_CORS_ORIGINS".to_owned(),
                "https://app.example.test".to_owned(),
            )],
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let key = Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await);
    for origin in [
        "https://evil.example.test",
        "null",
        "https://app.example.test:444",
    ] {
        let response = rpc(&h, &key, "tools/list", json!({}))
            .header("origin", origin)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "{origin}");
    }
    let allowed = rpc(&h, &key, "tools/list", json!({}))
        .header("origin", "https://app.example.test")
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status(), 200);
    assert_eq!(allowed.headers()["cache-control"], "no-store");
    for (origin, status) in [
        ("https://app.example.test", 200),
        ("https://evil.example.test", 403),
    ] {
        let preflight = h
            .http
            .request(reqwest::Method::OPTIONS, h.base.join("mcp").unwrap())
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "mcp-method,mcp-name,mcp-protocol-version,x-api-key",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(preflight.status(), status);
    }
    let foreign_host = rpc(&h, &key, "tools/list", json!({}))
        .header("host", "foreign.example.test")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign_host.status(), 403);
    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        let response = key
            .apply(h.http.request(method, h.base.join("mcp").unwrap()))
            .header("mcp-protocol-version", VERSION)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 405);
    }
    // The server rejects an oversized Content-Length before reading the body.
    // Read that final response before sending bytes: reqwest can otherwise
    // report BodyWrite/BrokenPipe instead of exposing the server's 413.
    // The chunked request below separately exercises actual body overflow.
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut socket = tokio::net::TcpStream::connect((
        h.base.host_str().unwrap(),
        h.base.port_or_known_default().unwrap(),
    ))
    .await
    .unwrap();
    let request = key.apply(h.http.post(h.url("/mcp"))).build().unwrap();
    let mut headers = format!(
        "POST /mcp HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        h.base.host_str().unwrap(),
        h.base.port_or_known_default().unwrap(),
        ariad_server::api::JSON_BODY_LIMIT + 1,
    );
    for (name, value) in request.headers() {
        headers.push_str(&format!("{name}: {}\r\n", value.to_str().unwrap()));
    }
    headers.push_str("\r\n");
    socket.write_all(headers.as_bytes()).await.unwrap();
    let mut response = BufReader::new(socket);
    let mut status_line = String::new();
    tokio::time::timeout(Duration::from_secs(5), response.read_line(&mut status_line))
        .await
        .expect("oversized Content-Length must be rejected without waiting for a body")
        .unwrap();
    assert_eq!(status_line.split_whitespace().nth(1), Some("413"));
    drop(response);
    let stream = futures_util::stream::iter([
        Ok::<_, std::io::Error>(bytes::Bytes::from(vec![
            b'x';
            ariad_server::api::JSON_BODY_LIMIT
        ])),
        Ok(bytes::Bytes::from_static(b"x")),
    ]);
    let chunked = rpc(&h, &key, "tools/list", json!({}))
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .unwrap();
    assert_eq!(chunked.status(), 413);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn inline_and_uploaded_conversions_share_the_worker_and_workspace_limits() {
    let Some(mut h) = Harness::start("mcp_conversion", Options::default()).await else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let alice = Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await);
    let bob = Auth::ApiKey(insert_api_key(&h.db.pool, "bob").await);
    h.start_worker();
    let engines = call(&h, &alice, "list_engines", json!({})).await;
    assert!(
        !engines["structuredContent"]["engines"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let plan = call(&h, &alice, "plan", json!({"from": "md", "to": "html"})).await;
    assert_ne!(plan["isError"], true, "{plan}");

    let inline = call(
        &h,
        &alice,
        "convert",
        json!({
            "inline": {"filename": "notes.md", "content": "# Remote MCP\n\nHello.\n"}, "to": "html"
        }),
    )
    .await;
    assert_ne!(inline["isError"], true, "{inline}");
    let job_id = inline["structuredContent"]["job"]["id"].as_str().unwrap();
    let terminal = wait_for_success(&h, &alice, job_id).await;
    let output = &terminal["structuredContent"]["outputs"][0];
    let html = h
        .http
        .get(output["url"].as_str().unwrap())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("Remote MCP"), "{html}");
    assert!(html.contains("Hello."), "{html}");
    let outsider = call(&h, &bob, "job_status", json!({"job_id": job_id})).await;
    assert_eq!(outsider["isError"], true);
    assert_eq!(outsider["structuredContent"]["error"]["code"], "not_found");

    let uploaded = h.upload(&alice, "uploaded.md", b"# Uploaded\n").await;
    let upload_id = uploaded["id"].as_str().unwrap();
    let inspected = call(&h, &alice, "inspect", json!({"upload_id": upload_id})).await;
    assert_eq!(inspected["structuredContent"]["format"], "markdown");
    let inaccessible = call(&h, &bob, "inspect", json!({"upload_id": upload_id})).await;
    assert_eq!(inaccessible["isError"], true);
    let converted = call(
        &h,
        &alice,
        "convert",
        json!({"upload_id": upload_id, "to": "html"}),
    )
    .await;
    assert_ne!(converted["isError"], true, "{converted}");
    let uploaded_job = converted["structuredContent"]["job"]["id"]
        .as_str()
        .unwrap();
    let uploaded_result = wait_for_success(&h, &alice, uploaded_job).await;
    let downloaded = h
        .http
        .get(
            uploaded_result["structuredContent"]["outputs"][0]["url"]
                .as_str()
                .unwrap(),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(downloaded.contains("Uploaded"), "{downloaded}");
    let invalid = call(
        &h,
        &alice,
        "convert",
        json!({
            "inline": {"filename": "notes.md", "content": "é".repeat(131073)}, "to": "html"
        }),
    )
    .await;
    assert_eq!(invalid["isError"], true);

    sqlx::query("UPDATE plan_entitlements SET jobs_per_day = 0 WHERE plan = 'free'")
        .execute(&h.db.pool)
        .await
        .unwrap();
    let refused = call(
        &h,
        &alice,
        "convert",
        json!({"upload_id": upload_id, "to": "html"}),
    )
    .await;
    assert_eq!(refused["isError"], true);
    assert_eq!(
        refused["structuredContent"]["error"]["code"],
        "quota_exceeded"
    );
    sqlx::query("UPDATE plan_entitlements SET requests_per_minute = 1 WHERE plan = 'free'")
        .execute(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(
        rpc(&h, &alice, "tools/list", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let limited = rpc(&h, &alice, "tools/list", json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(limited.status(), 429);
    assert!(limited.headers().contains_key("retry-after"));
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_fleet_refusal_preserves_upload_allowance_for_recovery() {
    let Some(mut h) = Harness::start(
        "mcp_inline_fleet",
        Options {
            restricted_logins: true,
            env: vec![("ARIAD_GLOBAL_ACTIVE_JOBS".into(), "1".into())],
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    let auth = Auth::ApiKey(insert_api_key(&h.db.pool, "inline-fleet").await);
    sqlx::query("UPDATE plan_entitlements SET jobs_per_day = 3 WHERE plan = 'free'")
        .execute(&h.db.pool)
        .await
        .unwrap();
    let args =
        json!({"inline": {"filename": "notes.md", "content": "# Fleet recovery"}, "to": "html"});
    let first = call(&h, &auth, "convert", args.clone()).await;
    assert_ne!(first["isError"], true, "{first}");
    let job_id = first["structuredContent"]["job"]["id"].as_str().unwrap();
    let refused = call(&h, &auth, "convert", args).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"],
        "quota_exceeded"
    );
    let upload_id = refused["structuredContent"]["upload_id"]
        .as_str()
        .expect("a quota refusal must return the reusable inline upload");
    let outsider = Auth::ApiKey(insert_api_key(&h.db.pool, "inline-outsider").await);
    let hidden = call(&h, &outsider, "inspect", json!({"upload_id": upload_id})).await;
    assert_eq!(hidden["structuredContent"]["error"]["code"], "not_found");
    let retry = json!({"upload_id": upload_id, "to": "html"});
    for _ in 0..3 {
        let refused = call(&h, &auth, "convert", retry.clone()).await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert!(refused.to_string().contains("fleet"), "{refused}");
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM uploads")
            .fetch_one(&h.db.pool)
            .await
            .unwrap(),
        2,
        "retries must reuse the automatic upload"
    );
    ariad_server::db::cancel_job(
        &mut h.db.pool.acquire().await.unwrap(),
        uuid::Uuid::parse_str(job_id).unwrap(),
    )
    .await
    .unwrap();
    let recovered = call(&h, &auth, "convert", retry).await;
    assert_ne!(recovered["isError"], true, "{recovered}");
    h.start_worker();
    wait_for_success(
        &h,
        &auth,
        recovered["structuredContent"]["job"]["id"]
            .as_str()
            .unwrap(),
    )
    .await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM uploads")
            .fetch_one(&h.db.pool)
            .await
            .unwrap(),
        2
    );
    // Failed automatic inputs are retired, but still count toward abuse limits.
    let rejected = call(
        &h,
        &auth,
        "convert",
        json!({
            "inline": {"filename": "bad.md", "content": "# No same-format route"}, "to": "md"
        }),
    )
    .await;
    assert_eq!(rejected["isError"], true, "{rejected}");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM uploads WHERE status='deleted'")
            .fetch_one(&h.db.pool)
            .await
            .unwrap(),
        1
    );
    let exhausted = call(
        &h,
        &auth,
        "convert",
        json!({
            "inline": {"filename": "next.md", "content": "# Over allowance"}, "to": "html"
        }),
    )
    .await;
    assert!(
        exhausted.to_string().contains("upload allowance"),
        "{exhausted}"
    );
    h.stop().await;
}
