//! Cloud tool admission and multi-input/multi-artifact worker execution against PostgreSQL.
mod common;

use common::{Auth, Harness, Options, insert_api_key};
use serde_json::{Value, json};
use std::time::Duration;

const PDF: &[u8] = include_bytes!("../../../fixtures/pdf/en-map-room.pdf");

async fn start(test: &str) -> Option<(Harness, Auth)> {
    let h = Harness::start(
        test,
        Options {
            restricted_logins: true,
            ..Default::default()
        },
    )
    .await?;
    let auth = Auth::ApiKey(insert_api_key(&h.db.pool, "tool-owner").await);
    Some((h, auth))
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_admission_checks_tiers_params_ownership_arity_and_quota_before_queueing() {
    let Some((h, auth)) = start("tool_admission").await else {
        return;
    };
    let upload = h.upload(&auth, "input.pdf", PDF).await;
    let other = Auth::ApiKey(insert_api_key(&h.db.pool, "other-owner").await);
    let foreign = h.upload(&other, "foreign.pdf", PDF).await;
    let pending = h.create_upload(&auth, "pending.pdf", PDF).await;
    let text = h.upload(&auth, "input.md", b"# Text\n").await;
    let pro_tool = ariad_core::tools::all()
        .iter()
        .find(|s| s.tier == ariad_core::tools::Tier::Pro)
        .unwrap();
    let cases = [
        (
            json!({"tool":"unknown.tool", "upload_id":upload["id"]}),
            422,
            "unknown_tool",
        ),
        (
            json!({"tool":pro_tool.id, "upload_id":upload["id"]}),
            403,
            "tier_not_allowed",
        ),
        (
            json!({"tool":"pdf.rotate", "upload_id":upload["id"], "params":{"angle":45}}),
            422,
            "invalid_params",
        ),
        (
            json!({"tool":"pdf.rotate", "upload_id":upload["id"], "params":[]}),
            422,
            "invalid_params",
        ),
        (
            json!({"tool":"pdf.decrypt", "upload_id":upload["id"], "params":{"password":"private-literal"}}),
            422,
            "invalid_params",
        ),
        (
            json!({"tool":"pdf.decrypt", "upload_id":upload["id"], "params":{"password":{"$secret":"sec_unresolved"}}}),
            422,
            "invalid_params",
        ),
        (
            json!({"tool":"pdf.metadata", "upload_id":upload["id"], "params":{"set":{"PASSWORD":"private-literal"}}}),
            422,
            "invalid_params",
        ),
        (
            json!({"tool":"pdf.rotate", "upload_id":foreign["id"], "params":{"angle":90}}),
            404,
            "not_found",
        ),
        (
            json!({"tool":"pdf.rotate", "upload_id":pending["id"], "params":{"angle":90}}),
            409,
            "conflict",
        ),
        (
            json!({"tool":"pdf.rotate", "upload_id":text["id"], "params":{"angle":90}}),
            422,
            "wrong_input",
        ),
        (
            json!({"tool":"pdf.merge", "upload_id":upload["id"]}),
            422,
            "wrong_input",
        ),
        (
            json!({"tool":"pdf.merge", "upload_ids":vec![upload["id"].clone();26]}),
            403,
            "tier_not_allowed",
        ),
        (json!({"upload_id":upload["id"]}), 422, "invalid_request"),
        (
            json!({"tool":"pdf.rotate", "upload_id":upload["id"], "upload_ids":[upload["id"]]}),
            422,
            "invalid_request",
        ),
        (
            json!({"tool":"pdf.rotate", "upload_id":upload["id"], "to":"pdf"}),
            422,
            "invalid_request",
        ),
    ];
    for (request, status, code) in cases {
        let response = h.post(&auth, "/v1/jobs", &request).await;
        assert_eq!(response.status().as_u16(), status, "{request}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], code, "{body}");
        assert!(!body.to_string().contains("private-literal"));
    }
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(jobs, 0);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn merge_stages_all_inputs_preserves_v1_fields_and_delete_retires_every_input() {
    let Some((mut h, auth)) = start("tool_merge_execution").await else {
        return;
    };
    let first = h.upload(&auth, "first.pdf", PDF).await;
    let second = h.upload(&auth, "second.pdf", PDF).await;
    let ids = json!([second["id"], first["id"]]);
    let response = h
        .post(
            &auth,
            "/v1/jobs",
            &json!({"tool":"pdf.merge", "upload_ids":ids, "params":{}}),
        )
        .await;
    assert_eq!(response.status(), 202);
    let job: Value = response.json().await.unwrap();
    assert_eq!(job["upload_id"], second["id"]);
    assert_eq!(job["upload_ids"], ids);
    assert_eq!(job["from"], "pdf");
    assert_eq!(job["to"], "pdf");
    assert_eq!(job["pack"], "core");
    for key in [
        "id",
        "status",
        "upload_id",
        "from",
        "to",
        "profile",
        "pack",
        "route",
        "warnings",
        "attempts",
        "created_at",
        "expires_at",
    ] {
        assert!(!job[key].is_null(), "missing {key}: {job}");
    }
    let id = job["id"].as_str().unwrap();
    let target: Option<String> = sqlx::query_scalar("SELECT target_format FROM jobs WHERE id=$1")
        .bind(id.parse::<uuid::Uuid>().unwrap())
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    assert!(target.is_none());
    let delete_secondary = h
        .delete(
            &auth,
            &format!("/v1/uploads/{}", first["id"].as_str().unwrap()),
        )
        .await;
    assert_eq!(
        delete_secondary.status(),
        409,
        "secondary inputs belong to the job too"
    );
    h.start_worker();
    let finished = h.wait_final(&auth, id, Duration::from_secs(60)).await;
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let outputs: Value = h
        .get(&auth, &format!("/v1/jobs/{id}/outputs"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(outputs["outputs"].as_array().unwrap().len(), 1);
    let output = h
        .http
        .get(outputs["outputs"][0]["url"].as_str().unwrap())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert!(output.starts_with(b"%PDF-"));
    assert_ne!(output.as_ref(), PDF);
    let deleted = h.delete(&auth, &format!("/v1/jobs/{id}")).await;
    assert_eq!(deleted.status(), 204);
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM uploads WHERE status = 'completed'")
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_corrupt_secondary_input_fails_without_publishing_artifacts() {
    let Some((mut h, auth)) = start("tool_secondary_integrity").await else {
        return;
    };
    let first = h.upload(&auth, "first.pdf", PDF).await;
    let second = h.upload(&auth, "second.pdf", PDF).await;
    let job: Value = h
        .post(
            &auth,
            "/v1/jobs",
            &json!({"tool":"pdf.merge", "upload_ids":[first["id"], second["id"]]}),
        )
        .await
        .json()
        .await
        .unwrap();
    let key: String = sqlx::query_scalar("SELECT storage_key FROM uploads WHERE id=$1")
        .bind(
            second["id"]
                .as_str()
                .unwrap()
                .parse::<uuid::Uuid>()
                .unwrap(),
        )
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    std::fs::write(h.objects.path().join(key), b"corrupt").unwrap();
    h.start_worker();
    let finished = h
        .wait_final(&auth, job["id"].as_str().unwrap(), Duration::from_secs(60))
        .await;
    assert_eq!(finished["status"], "failed", "{finished}");
    assert_eq!(finished["error"]["code"], "input_io");
    let artifacts: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(artifacts, 0);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn input_free_tools_keep_nonnullable_v1_identity_fields() {
    let Some((h, auth)) = start("tool_no_inputs").await else {
        return;
    };
    let response = h
        .post(
            &auth,
            "/v1/jobs",
            &json!({"tool":"web.to_pdf", "params":{"url":"https://example.test"}}),
        )
        .await;
    assert_eq!(response.status(), 202);
    let job: Value = response.json().await.unwrap();
    assert_eq!(job["upload_id"], uuid::Uuid::nil().to_string());
    assert_eq!(job["upload_ids"], json!([]));
    assert_eq!(job["from"], "none");
    assert_eq!(job["to"], "pdf");
    let persisted: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT upload_id FROM jobs WHERE id=$1")
            .bind(job["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap())
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
    assert!(persisted.is_none());
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pro_admission_snapshots_access_and_selects_the_tool_engine_queue() {
    let Some((h, auth)) = start("tool_pro_pack").await else {
        return;
    };
    let upload = h.upload(&auth, "input.pdf", PDF).await;
    sqlx::query("UPDATE workspaces SET plan='pro' WHERE owner_user_id='tool-owner'")
        .execute(&h.db.pool)
        .await
        .unwrap();
    // Core is the only enabled pack in this harness; Pro access is granted,
    // but a disabled engine is still refused before metering or enqueueing.
    let response = h
        .post(
            &auth,
            "/v1/jobs",
            &json!({"tool":"edit.page_objects", "upload_id":upload["id"]}),
        )
        .await;
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "tool_missing", "{body}");
    let response = h
        .post(
            &auth,
            "/v1/jobs",
            &json!({"tool":"pdf.rotate", "upload_id":upload["id"], "params":{"angle":90}}),
        )
        .await;
    assert_eq!(response.status(), 202);
    let job: Value = response.json().await.unwrap();
    let plan: String = sqlx::query_scalar("SELECT tool_plan FROM jobs WHERE id=$1")
        .bind(job["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap())
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(plan, "pro");
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn metadata_pruning_retains_secondary_uploads_until_their_jobs_are_removed() {
    let Some((h, auth)) = start("tool_secondary_retention").await else {
        return;
    };
    let first = h.upload(&auth, "first.pdf", PDF).await;
    let second = h.upload(&auth, "second.pdf", PDF).await;
    let job: Value = h
        .post(
            &auth,
            "/v1/jobs",
            &json!({"tool":"pdf.merge", "upload_ids":[first["id"], second["id"]]}),
        )
        .await
        .json()
        .await
        .unwrap();
    let second_id = second["id"]
        .as_str()
        .unwrap()
        .parse::<uuid::Uuid>()
        .unwrap();
    sqlx::query(
        "UPDATE uploads SET status='expired', expires_at=now()-interval '31 days' WHERE id=$1",
    )
    .bind(second_id)
    .execute(&h.db.pool)
    .await
    .unwrap();
    let storage = ariad_server::storage::Storage::from_config(
        &ariad_server::config::StorageConfig::Local {
            root: h.objects.path().to_path_buf(),
        },
        None,
        None,
    )
    .unwrap();
    ariad_server::sweeper::sweep_once(&h.db.pool, &storage)
        .await
        .unwrap();
    assert!(
        ariad_server::db::get_upload(&h.db.pool, second_id)
            .await
            .unwrap()
            .is_some()
    );
    let id = job["id"].as_str().unwrap();
    assert_eq!(
        h.delete(&auth, &format!("/v1/jobs/{id}")).await.status(),
        204
    );
    sqlx::query("UPDATE jobs SET expires_at=now()-interval '31 days' WHERE id=$1")
        .bind(id.parse::<uuid::Uuid>().unwrap())
        .execute(&h.db.pool)
        .await
        .unwrap();
    ariad_server::sweeper::sweep_once(&h.db.pool, &storage)
        .await
        .unwrap();
    assert!(
        ariad_server::db::get_upload(&h.db.pool, second_id)
            .await
            .unwrap()
            .is_none()
    );
    h.stop().await;
}
