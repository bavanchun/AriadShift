//! Migrations, the schema check, and seeded data.

mod common;

use ariad_core::limits::Limits;
use ariad_server::{db, entitlements};
use common::TestDb;

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_nonblocking_queue_read_does_not_long_poll() {
    use ariad_server::{
        packs::Pack,
        queue::{JobQueue, PgmqQueue},
    };
    let Some(test_db) = TestDb::create("an_empty_nonblocking_queue_read_does_not_long_poll").await
    else {
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '500ms'")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&test_db.url)
        .await
        .unwrap();
    let queue = PgmqQueue::new(pool.clone());
    // A long poll's one-second minimum would hit the database deadline;
    // nonblocking selection must return without reserving an idle worker slot.
    assert!(
        queue
            .read(
                Pack::Docling,
                std::time::Duration::from_secs(30),
                std::time::Duration::ZERO
            )
            .await
            .unwrap()
            .is_none()
    );
    pool.close().await;
    test_db.drop_database().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_database_roles_cannot_read_identity_secrets_or_migrate() {
    let Some(test_db) =
        TestDb::create("runtime_database_roles_cannot_read_identity_secrets_or_migrate").await
    else {
        return;
    };
    common::create_apikey_table(&test_db.pool).await;
    sqlx::raw_sql("CREATE TABLE jwks (private_key text); CREATE TABLE \"user\" (email text); CREATE TABLE session (token text); CREATE TABLE account (password text)")
        .execute(&test_db.pool).await.unwrap();
    test_db.apply_runtime_roles().await;
    for role in ["ariad_api", "ariad_worker"] {
        let mut conn = test_db.pool.acquire().await.unwrap();
        sqlx::raw_sql(if role == "ariad_api" {
            "SET ROLE ariad_api"
        } else {
            "SET ROLE ariad_worker"
        })
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query("SELECT * FROM jobs")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("SELECT * FROM _sqlx_migrations")
            .execute(&mut *conn)
            .await
            .unwrap();
        for query in [
            "SELECT * FROM jwks",
            "SELECT * FROM \"user\"",
            "SELECT * FROM session",
            "SELECT * FROM account",
            "CREATE TABLE forbidden (id int)",
            "SELECT pgmq.create('forbidden')",
            "UPDATE plan_entitlements SET max_memory_mb = 1",
        ] {
            assert!(
                sqlx::raw_sql(query).execute(&mut *conn).await.is_err(),
                "{role}: {query}"
            );
        }
        if role == "ariad_worker" {
            for table in ["workspaces", "rate_limit_counters", "event_stream_leases"] {
                assert!(
                    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
                        .execute(&mut *conn)
                        .await
                        .is_err()
                );
            }
        }
        let apikey = sqlx::query("SELECT * FROM apikey")
            .execute(&mut *conn)
            .await;
        assert_eq!(apikey.is_ok(), role == "ariad_api");
        let id: i64 = sqlx::query_scalar(
            "SELECT pgmq.send('jobs_core', '{\"job_id\": null}'::jsonb, '{}'::jsonb)",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        sqlx::query("SELECT * FROM pgmq.read('jobs_core', 30, 1)")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("SELECT pgmq.set_vt('jobs_core', $1, 30)")
            .bind(id)
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("SELECT pgmq.archive('jobs_core', $1)")
            .bind(id)
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("SELECT * FROM pgmq.metrics('jobs_core')")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::raw_sql("RESET ROLE")
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    test_db.drop_database().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn services_refuse_an_unmigrated_database() {
    let Some(test_db) = TestDb::create_empty("services_refuse_an_unmigrated_database").await else {
        return;
    };
    assert!(matches!(
        db::check_schema(&test_db.pool).await,
        Err(db::SchemaError::Pending)
    ));
    db::migrate(&test_db.pool).await.expect("migrate");
    db::check_schema(&test_db.pool)
        .await
        .expect("schema is current");
    db::migrate(&test_db.pool)
        .await
        .expect("migrating again is a no-op");

    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (99991231000000, 'from a newer build', true, '\\x00', 0)",
    )
    .execute(&test_db.pool)
    .await
    .expect("fake a newer migration");
    assert!(matches!(
        db::check_schema(&test_db.pool).await,
        Err(db::SchemaError::Unknown(99_991_231_000_000))
    ));
    test_db.drop_database().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn migrations_seed_plans_queues_and_the_anonymous_tenant() {
    let Some(test_db) =
        TestDb::create("migrations_seed_plans_queues_and_the_anonymous_tenant").await
    else {
        return;
    };
    let free = entitlements::load(&test_db.pool, "free")
        .await
        .expect("free");
    assert_eq!(
        free.limits,
        Limits::cloud(),
        "free carries the cloud defaults"
    );
    assert_eq!(free.retention.as_secs(), 3600);
    let anonymous = entitlements::load(&test_db.pool, "anonymous")
        .await
        .expect("anonymous");
    assert_eq!(anonymous.limits.max_input_bytes, Some(10 * 1024 * 1024));
    assert!(anonymous.requests_per_minute.is_some());
    assert!(matches!(
        entitlements::load(&test_db.pool, "enterprise").await,
        Err(entitlements::EntitlementError::UnknownPlan(_))
    ));

    let queues: Vec<String> =
        sqlx::query_scalar("SELECT queue_name FROM pgmq.list_queues() ORDER BY queue_name")
            .fetch_all(&test_db.pool)
            .await
            .expect("queues");
    assert_eq!(queues, ["jobs_core", "jobs_docling", "jobs_office"]);

    let tenant: String = sqlx::query_scalar("SELECT plan FROM workspaces WHERE id = $1")
        .bind(db::ANONYMOUS_WORKSPACE)
        .fetch_one(&test_db.pool)
        .await
        .expect("anonymous workspace");
    assert_eq!(tenant, "anonymous");

    // The job status check rejects states outside the lifecycle.
    let err = sqlx::query("UPDATE jobs SET status = 'paused'")
        .execute(&test_db.pool)
        .await;
    assert!(err.is_ok(), "no rows yet, so nothing violates the check");
    let constraint: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint WHERE conrelid = 'jobs'::regclass AND contype = 'c' \
         AND pg_get_constraintdef(oid) LIKE '%expired%'",
    )
    .fetch_one(&test_db.pool)
    .await
    .expect("constraint");
    assert_eq!(constraint, 1);

    // Better Auth's tables are never created here.
    let foreign: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_name IN ('user', 'session', 'account', 'verification', 'apikey', 'jwks')",
    )
    .fetch_one(&test_db.pool)
    .await
    .expect("tables");
    assert_eq!(foreign, 0);
    test_db.drop_database().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restricted_logins_run_the_upload_plan_job_and_download_path() {
    use common::{Auth, Harness, Options};
    use serde_json::{Value, json};
    use std::time::Duration;
    let Some(mut h) = Harness::start(
        "restricted_logins_run_the_upload_plan_job_and_download_path",
        Options {
            restricted_logins: true,
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    h.start_worker();
    let key = common::insert_api_key(&h.db.pool, "restricted-runtime").await;
    let auth = Auth::ApiKey(key);
    let upload = h
        .upload(
            &auth,
            "restricted.md",
            b"# Restricted roles\n\nReal conversion.\n",
        )
        .await;
    let plan = h
        .post(
            &auth,
            "/v1/plan",
            &json!({"upload_id":upload["id"], "to":"docx"}),
        )
        .await;
    assert_eq!(plan.status(), 200, "{}", common::text(plan).await);
    let response = h
        .create_job(&auth, upload["id"].as_str().unwrap(), "docx")
        .await;
    assert_eq!(response.status(), 202);
    let job: Value = response.json().await.unwrap();
    let id = job["id"].as_str().unwrap();
    let job = h.wait_final(&auth, id, Duration::from_secs(60)).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    let outputs: Value = h
        .get(&auth, &format!("/v1/jobs/{id}/outputs"))
        .await
        .json()
        .await
        .unwrap();
    let output = h
        .http
        .get(outputs["outputs"][0]["url"].as_str().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(output.status(), 200);
    assert!(output.bytes().await.unwrap().starts_with(b"PK"));
    h.stop().await;
}
