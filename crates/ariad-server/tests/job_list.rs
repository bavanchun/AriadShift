//! `GET /v1/jobs`: the caller's own jobs, newest first, in stable pages.

mod common;

use common::{Auth, Harness, Options, TestIssuer, create_apikey_table, insert_api_key, text};
use serde_json::Value;

const MARKDOWN: &[u8] = b"# Listed\n\nBody.\n";

async fn start(test: &str) -> Option<(Harness, TestIssuer)> {
    let issuer = TestIssuer::start().await;
    let Some(h) = Harness::start(
        test,
        Options {
            env: issuer.env(),
            ..Options::default()
        },
    )
    .await
    else {
        issuer.stop();
        return None;
    };
    Some((h, issuer))
}

async fn error(response: reqwest::Response) -> (u16, String) {
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

/// Creates a queued job from a new upload; returns its id.
async fn job(h: &Harness, auth: &Auth, filename: &str) -> String {
    let upload = h.upload(auth, filename, MARKDOWN).await;
    job_from(h, auth, upload["id"].as_str().expect("upload id")).await
}

async fn job_from(h: &Harness, auth: &Auth, upload_id: &str) -> String {
    let response = h.create_job(auth, upload_id, "html").await;
    assert_eq!(
        response.status(),
        202,
        "create job: {}",
        text(response).await
    );
    let body: Value = response.json().await.expect("job JSON");
    body["id"].as_str().expect("job id").to_owned()
}

/// Moves a job's creation time to `secs_ago` seconds before an hour ago, so
/// the expected order does not depend on request timing and jobs created
/// later in the test are newer.
async fn created_secs_ago(h: &Harness, id: &str, secs_ago: i64) {
    sqlx::query(
        "UPDATE jobs SET created_at = now() - interval '1 hour' \
         - make_interval(secs => $2::double precision) WHERE id = $1::uuid",
    )
    .bind(id)
    .bind(secs_ago as f64)
    .execute(&h.db.pool)
    .await
    .expect("set created_at");
}

async fn list(h: &Harness, auth: &Auth, query: &str) -> Value {
    let response = h.get(auth, &format!("/v1/jobs{query}")).await;
    assert_eq!(
        response.status(),
        200,
        "list {query}: {}",
        text(response).await
    );
    response.json().await.expect("list JSON")
}

fn ids(page: &Value) -> Vec<String> {
    page["jobs"]
        .as_array()
        .expect("jobs array")
        .iter()
        .map(|job| job["id"].as_str().expect("id").to_owned())
        .collect()
}

/// Follows `next_cursor` from the first page; returns every id in order.
async fn all_pages(h: &Harness, auth: &Auth, query: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let mut page = list(h, auth, &format!("?{query}")).await;
    loop {
        seen.extend(ids(&page));
        let Some(cursor) = page["next_cursor"].as_str() else {
            assert!(page["next_cursor"].is_null());
            return seen;
        };
        page = list(h, auth, &format!("?{query}&cursor={cursor}")).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn job_lists_show_only_the_callers_jobs() {
    let Some((h, issuer)) = start("job_lists_show_only_the_callers_jobs").await else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let alice = Auth::Bearer(issuer.token("alice"));
    let alice_key = Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await);
    let bob = Auth::Bearer(issuer.token("bob"));

    let first = job(&h, &alice, "first.md").await;
    let second = job(&h, &alice_key, "second.md").await;
    created_secs_ago(&h, &first, 20).await;
    created_secs_ago(&h, &second, 10).await;
    let bobs = job(&h, &bob, "bob.md").await;
    let anonymous = job(&h, &Auth::Anonymous, "anonymous.md").await;

    // Both of Alice's credentials see her workspace, newest first.
    for auth in [&alice, &alice_key] {
        let page = list(&h, auth, "").await;
        assert_eq!(ids(&page), [second.clone(), first.clone()]);
        assert!(page["next_cursor"].is_null());
        assert_eq!(page["jobs"][0]["filename"], "second.md");
        assert_eq!(page["jobs"][1]["filename"], "first.md");
    }
    let page = list(&h, &bob, "").await;
    assert_eq!(ids(&page), std::slice::from_ref(&bobs));

    // A listed job is the `GET /v1/jobs/{id}` job plus its file name.
    let page = list(&h, &alice, "").await;
    for listed in page["jobs"].as_array().expect("jobs") {
        let mut listed = listed.clone();
        listed
            .as_object_mut()
            .expect("job object")
            .remove("filename");
        let id = listed["id"].as_str().expect("id");
        let single: Value = h
            .get(&alice, &format!("/v1/jobs/{id}"))
            .await
            .json()
            .await
            .expect("job JSON");
        assert_eq!(listed, single);
        assert!(!single["route"].as_array().expect("route").is_empty());
    }

    // Anonymous callers cannot list anything, not even anonymous jobs.
    assert_eq!(
        error(h.get(&Auth::Anonymous, "/v1/jobs").await).await,
        (401, "unauthorized".to_owned())
    );
    assert_eq!(
        error(h.get(&Auth::Anonymous, "/v1/jobs?limit=0").await).await,
        (401, "unauthorized".to_owned()),
        "credentials are checked before the query"
    );
    assert_eq!(
        error(
            h.get(&Auth::Bearer("not-a-jwt".to_owned()), "/v1/jobs")
                .await
        )
        .await,
        (401, "unauthorized".to_owned())
    );
    let workspaces: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT workspace_id::text FROM jobs WHERE id = ANY($1::uuid[])",
    )
    .bind([&first, &bobs, &anonymous].map(String::as_str).to_vec())
    .fetch_all(&h.db.pool)
    .await
    .expect("workspaces");
    assert_eq!(workspaces.len(), 3, "three tenants, each listed alone");

    // A new account starts with an empty list.
    let carol = Auth::Bearer(issuer.token("carol"));
    let page = list(&h, &carol, "").await;
    assert_eq!(page["jobs"], Value::Array(Vec::new()));
    assert!(page["next_cursor"].is_null());

    // Deleted jobs stay listed as expired, without the forgotten file name.
    let deleted = h.delete(&alice, &format!("/v1/jobs/{first}")).await;
    assert_eq!(deleted.status(), 204, "{}", text(deleted).await);
    let page = list(&h, &alice, "").await;
    assert_eq!(ids(&page), [second.clone(), first.clone()]);
    assert_eq!(page["jobs"][1]["status"], "expired");
    assert!(
        page["jobs"][1].get("filename").is_none(),
        "{}",
        page["jobs"][1]
    );

    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn job_list_pages_stay_stable_across_inserts() {
    let Some((h, issuer)) = start("job_list_pages_stay_stable_across_inserts").await else {
        return;
    };
    let alice = Auth::Bearer(issuer.token("alice"));
    let upload = h.upload(&alice, "pages.md", MARKDOWN).await;
    let upload_id = upload["id"].as_str().expect("id");
    let mut jobs = Vec::new();
    for secs_ago in [50, 40, 30, 20, 10] {
        let id = job_from(&h, &alice, upload_id).await;
        created_secs_ago(&h, &id, secs_ago).await;
        jobs.push(id);
    }
    jobs.reverse(); // Newest first.

    let page = list(&h, &alice, "?limit=2").await;
    assert_eq!(ids(&page), jobs[..2]);
    let cursor = page["next_cursor"].as_str().expect("more pages").to_owned();

    // A job created between pages appears on a new first page only.
    let newer = job_from(&h, &alice, upload_id).await;
    let page = list(&h, &alice, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(ids(&page), jobs[2..4]);
    let cursor = page["next_cursor"].as_str().expect("more pages").to_owned();
    let page = list(&h, &alice, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(ids(&page), jobs[4..]);
    assert!(page["next_cursor"].is_null(), "last page");
    assert_eq!(list(&h, &alice, "?limit=1").await["jobs"][0]["id"], newer);

    // Jobs created in the same instant are ordered by id and never skipped
    // or repeated across page boundaries.
    sqlx::query("UPDATE jobs SET created_at = timestamptz '2026-10-25 12:00:00+00'")
        .execute(&h.db.pool)
        .await
        .expect("tie every job");
    let mut expected: Vec<String> =
        sqlx::query_scalar("SELECT id::text FROM jobs ORDER BY id DESC")
            .fetch_all(&h.db.pool)
            .await
            .expect("ids");
    assert_eq!(expected.len(), 6);
    for limit in [1, 2, 4, 6, 7] {
        assert_eq!(
            all_pages(&h, &alice, &format!("limit={limit}")).await,
            expected,
            "limit {limit}"
        );
    }
    expected.sort();
    let mut seen = all_pages(&h, &alice, "limit=5").await;
    seen.sort();
    seen.dedup();
    assert_eq!(seen, expected);

    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn job_list_limits_are_bounded() {
    let Some((h, issuer)) = start("job_list_limits_are_bounded").await else {
        return;
    };
    let alice = Auth::Bearer(issuer.token("alice"));
    let template = job(&h, &alice, "many.md").await;
    // 120 jobs in all: more than the largest page.
    sqlx::query(
        "INSERT INTO jobs (id, workspace_id, subject, upload_id, source_format, target_format, \
         profile, pack, status, limits, created_at, expires_at) \
         SELECT gen_random_uuid(), workspace_id, subject, upload_id, source_format, \
         target_format, profile, pack, 'succeeded', limits, \
         created_at - make_interval(secs => n::double precision), expires_at \
         FROM jobs, generate_series(1, 119) AS n WHERE id = $1::uuid",
    )
    .bind(&template)
    .execute(&h.db.pool)
    .await
    .expect("insert jobs");

    let page = list(&h, &alice, "").await;
    assert_eq!(ids(&page).len(), 50, "default page size");
    assert!(page["next_cursor"].is_string());
    let page = list(&h, &alice, "?limit=100").await;
    assert_eq!(ids(&page).len(), 100, "largest page");
    let page = list(&h, &alice, "?limit=1").await;
    assert_eq!(ids(&page), std::slice::from_ref(&template));
    assert_eq!(all_pages(&h, &alice, "limit=100").await.len(), 120);

    for query in [
        "?limit=0",
        "?limit=101",
        "?limit=-1",
        "?limit=ten",
        "?cursor=",
        "?cursor=not-a-cursor",
        "?cursor=AAAA",
        "?status=unknown",
    ] {
        assert_eq!(
            error(h.get(&alice, &format!("/v1/jobs{query}")).await).await,
            (422, "invalid_request".to_owned()),
            "{query}"
        );
    }

    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn job_lists_filter_by_status() {
    let Some((h, issuer)) = start("job_lists_filter_by_status").await else {
        return;
    };
    let alice = Auth::Bearer(issuer.token("alice"));
    let bob = Auth::Bearer(issuer.token("bob"));
    let queued = job(&h, &alice, "queued.md").await;
    let failed = job(&h, &alice, "failed.md").await;
    let failed_too = job(&h, &alice, "failed-too.md").await;
    let succeeded = job(&h, &alice, "succeeded.md").await;
    let bobs_failed = job(&h, &bob, "bob.md").await;
    for (id, secs_ago) in [
        (&queued, 40),
        (&failed, 30),
        (&failed_too, 20),
        (&succeeded, 10),
    ] {
        created_secs_ago(&h, id, secs_ago).await;
    }
    sqlx::query(
        "UPDATE jobs SET status = 'failed', finished_at = now(), error_code = 'engine_failed', \
         error_message = 'failed' WHERE id = ANY($1::uuid[])",
    )
    .bind(vec![
        failed.as_str(),
        failed_too.as_str(),
        bobs_failed.as_str(),
    ])
    .execute(&h.db.pool)
    .await
    .expect("fail jobs");
    sqlx::query("UPDATE jobs SET status = 'succeeded', finished_at = now() WHERE id = $1::uuid")
        .bind(&succeeded)
        .execute(&h.db.pool)
        .await
        .expect("succeed job");

    let page = list(&h, &alice, "?status=failed").await;
    assert_eq!(ids(&page), [failed_too.clone(), failed.clone()]);
    assert_eq!(page["jobs"][0]["error"]["code"], "engine_failed");
    assert_eq!(ids(&list(&h, &alice, "?status=queued").await), [queued]);
    assert_eq!(
        ids(&list(&h, &alice, "?status=succeeded").await),
        [succeeded]
    );
    assert!(ids(&list(&h, &alice, "?status=canceled").await).is_empty());

    // The filter holds across pages.
    let page = list(&h, &alice, "?status=failed&limit=1").await;
    assert_eq!(ids(&page), [failed_too]);
    let cursor = page["next_cursor"].as_str().expect("second failed job");
    let page = list(
        &h,
        &alice,
        &format!("?status=failed&limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(ids(&page), [failed]);
    assert!(page["next_cursor"].is_null());

    h.stop().await;
    issuer.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn job_lists_are_rate_limited() {
    let Some((h, issuer)) = start("job_lists_are_rate_limited").await else {
        return;
    };
    h.sql_exec("UPDATE plan_entitlements SET requests_per_minute = 2 WHERE plan = 'free'")
        .await;
    let alice = Auth::Bearer(issuer.token("alice"));
    // Stay inside one minute window, so the count cannot reset midway.
    let into_minute = chrono::Utc::now().timestamp() % 60;
    if into_minute > 50 {
        tokio::time::sleep(std::time::Duration::from_secs((61 - into_minute) as u64)).await;
    }
    list(&h, &alice, "").await;
    list(&h, &alice, "").await;
    let limited = h.get(&alice, "/v1/jobs").await;
    assert!(limited.headers().contains_key("retry-after"));
    assert_eq!(error(limited).await, (429, "rate_limited".to_owned()));

    h.stop().await;
    issuer.stop();
}
