//! Real pgmq fairness across subjects, packs and independent database pools.
mod common;
use ariad_server::{
    db,
    packs::Pack,
    queue::{JobMessage, JobQueue, PgmqQueue},
};
use common::{Harness, Options};
use sqlx::Connection;
use std::{collections::BTreeMap, time::Duration};
use uuid::Uuid;

async fn start(test: &str) -> Option<Harness> {
    Harness::start(
        test,
        Options {
            restricted_logins: true,
            ..Default::default()
        },
    )
    .await
}

async fn enqueue(h: &Harness, subject: &str, pack: Pack) -> Uuid {
    let workspace_id = db::ensure_workspace(&h.db.pool, subject).await.unwrap().0;
    let id = Uuid::new_v4();
    let upload = Uuid::new_v4();
    let mut tx = h.db.pool.begin().await.unwrap();
    db::insert_upload(
        &mut tx,
        &db::NewUpload {
            id: upload,
            workspace_id,
            subject,
            filename: "test.md",
            size_bytes: 0,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            storage_key: "synthetic-empty",
            retention_s: 3600,
        },
    )
    .await
    .unwrap();
    db::insert_job(
        &mut tx,
        &db::NewJob {
            id,
            workspace_id,
            subject,
            upload_id: upload,
            source_format: "markdown",
            target_format: "html",
            profile: "editable",
            pack,
            limits: serde_json::to_value(ariad_core::limits::Limits::default()).unwrap(),
            retention_s: 3600,
        },
    )
    .await
    .unwrap();
    PgmqQueue::new(h.db.pool.clone())
        .send(
            &mut tx,
            pack,
            &JobMessage { job_id: id },
            &BTreeMap::from([("traceparent".into(), "original-context".into())]),
        )
        .await
        .unwrap();
    sqlx::query("UPDATE uploads SET status='completed', format='markdown' WHERE id=$1")
        .bind(upload)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    std::fs::write(h.objects.path().join("synthetic-empty"), "").unwrap();
    id
}

async fn read(queue: &PgmqQueue, pack: Pack) -> ariad_server::queue::Delivery {
    tokio::time::timeout(
        Duration::from_secs(3),
        queue.read(pack, Duration::from_secs(30), Duration::ZERO),
    )
    .await
    .unwrap()
    .unwrap()
    .expect("ready delivery")
}

#[tokio::test]
async fn later_subject_runs_before_single_worker_backlog_drains() {
    let Some(mut h) = start("fair_single_worker").await else {
        return;
    };
    let mut a = Vec::new();
    for _ in 0..4 {
        a.push(enqueue(&h, "a", Pack::Core).await);
    }
    let mut barrier = h.db.pool.acquire().await.unwrap().detach();
    sqlx::query("SELECT pg_advisory_lock(918001)")
        .execute(&mut barrier)
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION start_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN          IF NEW.kind='started' AND NEW.job_id='{}'::uuid THEN PERFORM pg_advisory_xact_lock(918001); END IF;          RETURN NEW; END $$; CREATE TRIGGER start_barrier BEFORE INSERT ON job_events          FOR EACH ROW EXECUTE FUNCTION start_barrier();", a[0]
    ))).execute(&h.db.pool).await.unwrap();
    h.start_worker();
    wait_running(&h, a[0]).await;
    let b = enqueue(&h, "b", Pack::Core).await;
    sqlx::query("SELECT pg_advisory_unlock(918001)")
        .execute(&mut barrier)
        .await
        .unwrap();
    barrier.close().await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if db::get_job(&h.db.pool, b).await.unwrap().unwrap().attempts > 0 {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "B never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    h.stop_workers().await;
    let order: Vec<Uuid> =
        sqlx::query_scalar("SELECT job_id FROM job_events WHERE kind='started' ORDER BY id")
            .fetch_all(&h.db.pool)
            .await
            .unwrap();
    assert!(
        order.iter().position(|id| *id == b).unwrap() < a.len(),
        "FIFO starved B: {order:?}"
    );
    h.stop().await;
}

#[tokio::test]
async fn replicas_share_turns_across_packs_and_keep_delivery_metadata() {
    let Some(h) = start("fair_replicas_packs").await else {
        return;
    };
    let a = enqueue(&h, "a", Pack::Core).await;
    let office_a = enqueue(&h, "a", Pack::Office).await;
    let office_b = enqueue(&h, "b", Pack::Office).await;
    let first = PgmqQueue::new(h.db.pool.clone());
    let second_pool = db::connect(&h.db.url, 1).await.unwrap();
    let second = PgmqQueue::new(second_pool.clone());
    let delivery = read(&first, Pack::Core).await;
    assert_eq!(delivery.message.unwrap().job_id, a);
    assert_eq!(delivery.read_count, 1);
    let stored_headers: sqlx::types::Json<BTreeMap<String, String>> =
        sqlx::query_scalar("SELECT headers FROM pgmq.q_jobs_core WHERE msg_id=$1")
            .bind(delivery.msg_id)
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
    assert_eq!(
        stored_headers.0.get("traceparent").unwrap(),
        "original-context"
    );
    assert_eq!(
        delivery.headers.get("traceparent").unwrap(),
        "original-context"
    );
    let delivery = read(&second, Pack::Office).await;
    assert_eq!(
        delivery.message.unwrap().job_id,
        office_b,
        "cross-pack A must yield to B"
    );
    assert_eq!(
        db::get_job(&h.db.pool, office_a)
            .await
            .unwrap()
            .unwrap()
            .attempts,
        0
    );
    second_pool.close().await;
    h.stop().await;
}

#[tokio::test]
async fn canceled_and_invalid_messages_are_archived_without_attempts() {
    let Some(mut h) = start("fair_invalid_canceled").await else {
        return;
    };
    let id = enqueue(&h, "a", Pack::Core).await;
    db::cancel_job(&mut h.db.pool.acquire().await.unwrap(), id)
        .await
        .unwrap();
    sqlx::query("SELECT pgmq.send('jobs_core', $1::jsonb)")
        .bind(serde_json::json!({"job_id": "invalid"}))
        .execute(&h.db.pool)
        .await
        .unwrap();
    h.start_worker();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM pgmq.q_jobs_core")
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
        if n == 0 {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        db::get_job(&h.db.pool, id).await.unwrap().unwrap().attempts,
        0
    );
    h.stop().await;
}

async fn wait_running(h: &Harness, id: Uuid) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if db::get_job(&h.db.pool, id).await.unwrap().unwrap().status == "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("worker reached the start barrier");
}

#[tokio::test]
async fn leased_duplicate_does_not_spend_attempts_or_block_another_subject() {
    let Some(mut h) = start("fair_duplicate").await else {
        return;
    };
    let a = enqueue(&h, "a", Pack::Core).await;
    let b = enqueue(&h, "b", Pack::Core).await;
    let mut owner = h.db.pool.acquire().await.unwrap().detach();
    sqlx::query("SELECT pg_advisory_lock(hashtextextended($1,0))")
        .bind(format!("ariad-job:{a}"))
        .execute(&mut owner)
        .await
        .unwrap();
    let queue = PgmqQueue::new(h.db.pool.clone());
    let mut tx = h.db.pool.begin().await.unwrap();
    queue
        .send(
            &mut tx,
            Pack::Core,
            &JobMessage { job_id: a },
            &BTreeMap::new(),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    h.start_worker();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if db::get_job(&h.db.pool, b).await.unwrap().unwrap().attempts > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a leased duplicate must not monopolize the worker");
    assert_eq!(
        db::get_job(&h.db.pool, a).await.unwrap().unwrap().attempts,
        0
    );
    owner.close().await.unwrap();
    h.stop().await;
}

#[tokio::test]
async fn worker_shutdown_returns_the_job_without_spending_an_attempt() {
    let Some(mut h) = start("fair_shutdown").await else {
        return;
    };
    let id = enqueue(&h, "shutdown", Pack::Core).await;
    let mut barrier = h.db.pool.acquire().await.unwrap().detach();
    sqlx::query("SELECT pg_advisory_lock(918002)")
        .execute(&mut barrier)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION shutdown_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN         IF NEW.kind='started' THEN PERFORM pg_advisory_xact_lock(918002); END IF; RETURN NEW; END $$;         CREATE TRIGGER shutdown_barrier BEFORE INSERT ON job_events FOR EACH ROW EXECUTE FUNCTION shutdown_barrier();")
        .execute(&h.db.pool).await.unwrap();
    h.start_worker();
    wait_running(&h, id).await;
    // stop_workers cancels first, then awaits the handler. The independent
    // barrier releases only after cancellation has reached the worker task.
    let release = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        sqlx::query("SELECT pg_advisory_unlock(918002)")
            .execute(&mut barrier)
            .await
            .unwrap();
        barrier.close().await.unwrap();
    });
    tokio::time::timeout(Duration::from_secs(15), h.stop_workers())
        .await
        .unwrap();
    release.await.unwrap();
    let row = db::get_job(&h.db.pool, id).await.unwrap().unwrap();
    assert_eq!(row.status, "queued");
    assert_eq!(row.attempts, 0);
    let delivery = read(&PgmqQueue::new(h.db.pool.clone()), Pack::Core).await;
    assert_eq!(delivery.message.unwrap().job_id, id);
    h.stop().await;
}

struct WorkerChild(std::process::Child);
impl Drop for WorkerChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn dead_worker_session_releases_the_job_for_reclaim() {
    let Some(mut h) = start("fair_dead_worker").await else {
        return;
    };
    let id = enqueue(&h, "dead-worker", Pack::Core).await;
    let mut barrier = h.db.pool.acquire().await.unwrap().detach();
    sqlx::query("SELECT pg_advisory_lock(918003)")
        .execute(&mut barrier)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION death_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        IF NEW.kind='started' THEN PERFORM pg_advisory_xact_lock(918003); END IF; RETURN NEW; END $$; \
        CREATE TRIGGER death_barrier BEFORE INSERT ON job_events FOR EACH ROW EXECUTE FUNCTION death_barrier();")
        .execute(&h.db.pool).await.unwrap();
    let work = tempfile::tempdir().unwrap();
    let mut child = WorkerChild(
        std::process::Command::new(env!("CARGO_BIN_EXE_ariad-server"))
            .arg("worker")
            .env_clear()
            .env("DATABASE_URL", &h.db.url)
            .env("ARIAD_WORK_DIR", work.path())
            .env("ARIAD_ENGINE_PROGRAM", common::ashift())
            .env("ARIAD_STORAGE_URL", &h.storage_env[0].1)
            .env("ARIAD_SANDBOX", "off")
            .env("ARIAD_WORKER_POLL_S", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_running(&h, id).await;
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    sqlx::query("SELECT pg_advisory_unlock(918003)")
        .execute(&mut barrier)
        .await
        .unwrap();
    barrier.close().await.unwrap();
    sqlx::query("DROP TRIGGER death_barrier ON job_events")
        .execute(&h.db.pool)
        .await
        .unwrap();
    // Make the visibility lease expire deterministically instead of waiting
    // two minutes; session-lock release itself comes from real process death.
    sqlx::query("UPDATE pgmq.q_jobs_core SET vt=clock_timestamp()")
        .execute(&h.db.pool)
        .await
        .unwrap();
    h.start_worker();
    let row = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let row = db::get_job(&h.db.pool, id).await.unwrap().unwrap();
            if row.status == "succeeded" {
                break row;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("another worker reclaims the dead session's job");
    assert_eq!(row.attempts, 2);
    h.stop().await;
}

#[tokio::test]
async fn independent_worker_replicas_share_subject_turns_across_packs() {
    let Some(mut h) = start("fair_worker_replicas").await else {
        return;
    };
    let core_a = enqueue(&h, "a", Pack::Core).await;
    let office_a = enqueue(&h, "a", Pack::Office).await;
    let office_b = enqueue(&h, "b", Pack::Office).await;
    let mut barrier = h.db.pool.acquire().await.unwrap().detach();
    sqlx::query("SELECT pg_advisory_lock(918004)")
        .execute(&mut barrier)
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION replicas_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        IF NEW.kind='started' AND NEW.job_id IN ('{core_a}'::uuid,'{office_b}'::uuid) \
        THEN PERFORM pg_advisory_xact_lock(918004); END IF; RETURN NEW; END $$; \
        CREATE TRIGGER replicas_barrier BEFORE INSERT ON job_events FOR EACH ROW EXECUTE FUNCTION replicas_barrier();"
    ))).execute(&h.db.pool).await.unwrap();
    h.start_worker();
    wait_running(&h, core_a).await;
    let login = format!("scheduler_worker_{}", Uuid::new_v4().simple());
    let password = Uuid::new_v4().simple().to_string();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE ROLE \"{login}\" LOGIN PASSWORD '{password}' IN ROLE ariad_worker"
    )))
    .execute(&h.db.pool)
    .await
    .unwrap();
    let mut worker_url = url::Url::parse(&h.db.url).unwrap();
    worker_url.set_username(&login).unwrap();
    worker_url.set_password(Some(&password)).unwrap();
    let pool = db::connect(worker_url.as_str(), 1).await.unwrap();
    let work = tempfile::tempdir().unwrap();
    let config = ariad_server::config::WorkerConfig::from_env(
        &ariad_server::config::Env::from_pairs([
            ("ARIAD_SANDBOX", "off".to_owned()),
            ("ARIAD_STORAGE_URL", h.storage_env[0].1.clone()),
            ("ARIAD_WORK_DIR", work.path().display().to_string()),
            ("ARIAD_WORKER_POLL_S", "1".to_owned()),
            (
                "ARIAD_ENGINE_PROGRAM",
                common::ashift().display().to_string(),
            ),
            (
                "ARIAD_CONVERSION_PROGRAM",
                env!("CARGO_BIN_EXE_ariad-server").to_owned(),
            ),
        ]),
        &["office".to_owned()],
    )
    .unwrap();
    let stop = tokio_util::sync::CancellationToken::new();
    let worker = tokio::spawn(ariad_server::app::run_worker(
        config,
        pool.clone(),
        stop.clone(),
    ));
    wait_running(&h, office_b).await;
    let deferred = db::get_job(&h.db.pool, office_a).await.unwrap().unwrap();
    stop.cancel();
    sqlx::query("SELECT pg_advisory_unlock(918004)")
        .execute(&mut barrier)
        .await
        .unwrap();
    barrier.close().await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    pool.close().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP ROLE \"{login}\"")))
        .execute(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(
        deferred.attempts, 0,
        "A's office backlog must yield to B across replicas"
    );
    h.stop().await;
}

#[tokio::test]
async fn stale_backlog_cannot_hide_an_indexed_ready_job() {
    let Some(h) = start("fair_stale_backlog").await else {
        return;
    };
    sqlx::query(
        "SELECT pgmq.send('jobs_core', jsonb_build_object('bad',n)) FROM generate_series(1,300) n",
    )
    .execute(&h.db.pool)
    .await
    .unwrap();
    let id = enqueue(&h, "ready", Pack::Core).await;
    let queue = PgmqQueue::new(h.db.pool.clone());
    let delivery = read(&queue, Pack::Core).await;
    assert_eq!(
        delivery.message.unwrap().job_id,
        id,
        "stale messages must not hide ready subjects"
    );
    queue.archive(Pack::Core, delivery.msg_id).await.unwrap();
    for _ in 0..12 {
        queue
            .read(Pack::Core, Duration::from_secs(1), Duration::ZERO)
            .await
            .unwrap();
    }
    assert_eq!(queue.depth(Pack::Core).await.unwrap(), 0);
    let archived: i64 = sqlx::query_scalar("SELECT count(*) FROM pgmq.a_jobs_core")
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(archived, 301);
    let counted: i64 = sqlx::query_scalar("SELECT count(*) FROM pgmq.a_jobs_core WHERE read_ct=1")
        .fetch_one(&h.db.pool)
        .await
        .unwrap();
    assert_eq!(
        counted, 301,
        "archival retains one handled delivery per message"
    );
    h.stop().await;
}

#[tokio::test]
async fn newly_enqueued_work_inherits_its_subject_turn_and_oldest_job_wins() {
    let Some(h) = start("fair_inherited_turn").await else {
        return;
    };
    let first_a = enqueue(&h, "a", Pack::Core).await;
    let next_a = enqueue(&h, "a", Pack::Core).await;
    let b = enqueue(&h, "b", Pack::Core).await;
    let queue = PgmqQueue::new(h.db.pool.clone());
    assert_eq!(
        read(&queue, Pack::Core).await.message.unwrap().job_id,
        first_a
    );
    let late_a = enqueue(&h, "a", Pack::Core).await;
    assert_eq!(read(&queue, Pack::Core).await.message.unwrap().job_id, b);
    assert_eq!(
        read(&queue, Pack::Core).await.message.unwrap().job_id,
        next_a
    );
    assert_eq!(
        read(&queue, Pack::Core).await.message.unwrap().job_id,
        late_a
    );
    h.stop().await;
}

#[tokio::test]
async fn long_poll_releases_its_only_pool_connection_between_checks() {
    let Some(h) = start("fair_idle_pool").await else {
        return;
    };
    let pool = db::connect(&h.db.url, 1).await.unwrap();
    let queue = PgmqQueue::new(pool.clone());
    let polling = tokio::spawn(async move {
        queue
            .read(
                Pack::Docling,
                Duration::from_secs(30),
                Duration::from_secs(3),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let query = tokio::time::timeout(
        Duration::from_millis(750),
        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&pool),
    )
    .await;
    polling.abort();
    let _ = polling.await;
    pool.close().await;
    h.stop().await;
    assert_eq!(
        query
            .expect("long polling must not occupy the idle connection")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn existing_messages_are_discovered_without_a_schema_change() {
    let Some(h) = start("fair_existing_queue").await else {
        return;
    };
    let id = enqueue(&h, "legacy", Pack::Core).await;
    sqlx::query("UPDATE job_events SET data=data-'_queue_id' WHERE job_id=$1 AND kind='queued'")
        .bind(id)
        .execute(&h.db.pool)
        .await
        .unwrap();
    let delivery = read(&PgmqQueue::new(h.db.pool.clone()), Pack::Core).await;
    assert_eq!(delivery.message.unwrap().job_id, id);
    assert_eq!(delivery.read_count, 1);
    assert_eq!(
        delivery.headers.get("traceparent").unwrap(),
        "original-context"
    );
    h.stop().await;
}

#[tokio::test]
async fn returning_subject_cannot_reset_its_turn_ahead_of_existing_backlog() {
    let Some(h) = start("fair_returning_subject").await else {
        return;
    };
    let a = enqueue(&h, "a", Pack::Core).await;
    let first_b = enqueue(&h, "b", Pack::Core).await;
    let waiting_b = enqueue(&h, "b", Pack::Core).await;
    let queue = PgmqQueue::new(h.db.pool.clone());
    for id in [a, first_b] {
        let delivery = read(&queue, Pack::Core).await;
        assert_eq!(delivery.message.unwrap().job_id, id);
        db::cancel_job(&mut h.db.pool.acquire().await.unwrap(), id)
            .await
            .unwrap();
        queue.archive(Pack::Core, delivery.msg_id).await.unwrap();
    }
    enqueue(&h, "a", Pack::Core).await;
    let delivery = read(&queue, Pack::Core).await;
    assert_eq!(
        delivery.message.unwrap().job_id,
        waiting_b,
        "a returning subject must not obtain a fresh priority ahead of existing backlog"
    );
    h.stop().await;
}

#[tokio::test]
async fn terminal_leases_are_archived_without_redelivering_live_leases() {
    let Some(h) = start("fair_terminal_lease").await else {
        return;
    };
    let terminal = enqueue(&h, "terminal", Pack::Core).await;
    let live = enqueue(&h, "live", Pack::Core).await;
    let queue = PgmqQueue::new(h.db.pool.clone());
    let mut tx = h.db.pool.begin().await.unwrap();
    queue
        .send(
            &mut tx,
            Pack::Core,
            &JobMessage { job_id: live },
            &BTreeMap::new(),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let first = queue
        .read(Pack::Core, Duration::from_secs(300), Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    let second = queue
        .read(Pack::Core, Duration::from_secs(300), Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    let third = queue
        .read(Pack::Core, Duration::from_secs(300), Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(third.message.unwrap().job_id, live);
    assert_eq!(first.message.unwrap().job_id, terminal);
    assert_eq!(second.message.unwrap().job_id, live);
    db::cancel_job(&mut h.db.pool.acquire().await.unwrap(), terminal)
        .await
        .unwrap();
    assert!(
        queue
            .read(Pack::Core, Duration::from_secs(30), Duration::ZERO)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        queue.depth(Pack::Core).await.unwrap(),
        2,
        "terminal messages must not wait for visibility expiry"
    );
    for (sql, msg_id) in [
        (
            "SELECT read_ct FROM pgmq.a_jobs_core WHERE msg_id=$1",
            first.msg_id,
        ),
        (
            "SELECT read_ct FROM pgmq.q_jobs_core WHERE msg_id=$1",
            second.msg_id,
        ),
        (
            "SELECT read_ct FROM pgmq.q_jobs_core WHERE msg_id=$1",
            third.msg_id,
        ),
    ] {
        let read_count: i32 = sqlx::query_scalar(sql)
            .bind(msg_id)
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
        assert_eq!(
            read_count, 1,
            "maintenance must not debit an invisible delivery"
        );
    }
    assert_eq!(
        db::get_job(&h.db.pool, live)
            .await
            .unwrap()
            .unwrap()
            .attempts,
        0
    );
    h.stop().await;
}
