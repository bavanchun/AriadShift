//! Migrations, the schema check, and seeded data.

mod common;

use ariad_core::limits::Limits;
use ariad_server::{db, entitlements};
use common::TestDb;

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
