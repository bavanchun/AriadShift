//! Inline storage and completion errors retire uploads and release admission.

mod common;

use common::{Auth, Harness, Options, create_apikey_table, insert_api_key};
use serde_json::{Value, json};

async fn call(h: &Harness, auth: &Auth, name: &str, arguments: Value) -> Value {
    let response = auth
        .apply(h.http.post(h.url("/mcp")))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/call")
        .header("mcp-name", name)
        .json(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":name,"arguments":arguments,
                "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28", "io.modelcontextprotocol/clientInfo":{"name":"ariad-integration","version":"1"}, "io.modelcontextprotocol/clientCapabilities":{}}
            }}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert!(body.get("error").is_none(), "{body}");
    body["result"].clone()
}

async fn failed_upload_releases_quota(completion_failure: bool) {
    let Some(h) = Harness::start("inline_failure", Options::default()).await else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let auth = Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await);
    let engines = call(&h, &auth, "list_engines", json!({})).await;
    assert_ne!(engines["isError"], true, "{engines}");
    let workspace: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE owner_user_id = 'alice'")
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
    sqlx::query("UPDATE plan_entitlements SET concurrent_jobs = 1 WHERE plan = 'free'")
        .execute(&h.db.pool)
        .await
        .unwrap();
    // Inputs use t/<workspace>/<upload>/in/<hash>, verified against the row below.
    let blocked = h.objects.path().join("t").join(workspace.to_string());
    if completion_failure {
        sqlx::raw_sql("CREATE FUNCTION reject_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status = 'completed' THEN RAISE EXCEPTION 'completion rejected'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_completion BEFORE UPDATE ON uploads FOR EACH ROW EXECUTE FUNCTION reject_completion();")
            .execute(&h.db.pool).await.unwrap();
    } else {
        std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
        std::fs::write(&blocked, b"block object writes").unwrap();
    }
    let arguments = json!({"inline":{"filename":"notes.md","content":"# Inline\n"},"to":"html"});
    let failed = call(&h, &auth, "convert", arguments.clone()).await;
    assert_eq!(failed["isError"], true, "{failed}");
    assert_eq!(failed["structuredContent"]["status"], 500, "{failed}");
    assert_eq!(
        failed["structuredContent"]["error"]["code"], "internal",
        "{failed}"
    );
    let (id, key, status): (uuid::Uuid, String, String) =
        sqlx::query_as("SELECT id, storage_key, status FROM uploads WHERE workspace_id = $1")
            .bind(workspace)
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
    assert!(key.starts_with(&format!("t/{workspace}/{id}/in/")), "{key}");
    assert_eq!(
        status, "deleted",
        "failed inline upload must retire immediately"
    );
    if completion_failure {
        assert!(
            !h.objects.path().join(&key).exists(),
            "completion failure must remove stored bytes"
        );
        sqlx::raw_sql(
            "DROP TRIGGER reject_completion ON uploads; DROP FUNCTION reject_completion();",
        )
        .execute(&h.db.pool)
        .await
        .unwrap();
    } else {
        std::fs::remove_file(blocked).unwrap();
    }
    let retried = call(&h, &auth, "convert", arguments).await;
    assert_ne!(
        retried["isError"], true,
        "retired uploads must free the only pending slot: {retried}"
    );
    assert!(
        retried["structuredContent"]["job"]["id"].is_string(),
        "{retried}"
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_store_error_retires_the_upload_and_preserves_the_error() {
    failed_upload_releases_quota(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_completion_error_retires_the_upload_and_removes_the_object() {
    failed_upload_releases_quota(true).await;
}
