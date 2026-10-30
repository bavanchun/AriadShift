//! Startup refuses published secret placeholders without exposing their values.
use ariad_server::config::{ApiConfig, Env, WorkerConfig};

fn pairs() -> Vec<(&'static str, String)> {
    vec![
        ("ARIAD_SECRET", uuid::Uuid::new_v4().simple().to_string()),
        ("ARIAD_STORAGE_URL", "s3://test".into()),
        ("ARIAD_ENGINE_PROGRAM", "ashift".into()),
        ("ARIAD_WORK_DIR", "/tmp/ariad-secret-policy".into()),
    ]
}

fn rejects_placeholder(name: &'static str, worker: bool) {
    for value in [
        "replace-with-a-unique-random-signing-secret",
        "  replace-with-a-unique-random-signing-secret  ",
    ] {
        let mut values = pairs();
        values.retain(|(key, _)| *key != name);
        values.push((name, value.into()));
        let env = Env::from_pairs(values);
        let error = if worker {
            WorkerConfig::from_env(&env, &[])
                .expect_err(name)
                .to_string()
        } else {
            ApiConfig::from_env(&env).expect_err(name).to_string()
        };
        assert!(error.contains(name), "{error}");
        assert!(!error.contains(value.trim()), "secret leaked");
    }
}
macro_rules! placeholder_test {
    ($test:ident, $name:literal, $worker:literal) => {
        #[test]
        fn $test() {
            rejects_placeholder($name, $worker);
        }
    };
}
placeholder_test!(api_signing_placeholder, "ARIAD_SECRET", false);
placeholder_test!(api_turnstile_placeholder, "ARIAD_TURNSTILE_SECRET", false);
placeholder_test!(api_s3_id_placeholder, "AWS_ACCESS_KEY_ID", false);
placeholder_test!(api_s3_secret_placeholder, "AWS_SECRET_ACCESS_KEY", false);
placeholder_test!(api_s3_session_placeholder, "AWS_SESSION_TOKEN", false);
placeholder_test!(worker_signing_placeholder, "ARIAD_SECRET", true);
placeholder_test!(worker_turnstile_placeholder, "ARIAD_TURNSTILE_SECRET", true);
placeholder_test!(worker_s3_id_placeholder, "AWS_ACCESS_KEY_ID", true);
placeholder_test!(worker_s3_secret_placeholder, "AWS_SECRET_ACCESS_KEY", true);
placeholder_test!(worker_s3_session_placeholder, "AWS_SESSION_TOKEN", true);

#[test]
fn provider_discovery_public_identifier_and_file_storage_remain_available() {
    let mut values = pairs();
    values.push(("AWS_ACCESS_KEY_ID", "ariadshift".into()));
    let env = Env::from_pairs(values.clone());
    ApiConfig::from_env(&env).unwrap();
    WorkerConfig::from_env(&env, &[]).unwrap();
    values.retain(|(key, _)| *key != "ARIAD_STORAGE_URL");
    values.push((
        "ARIAD_STORAGE_URL",
        "file:///tmp/ariad-secret-policy".into(),
    ));
    values.push(("ARIAD_PUBLIC_URL", "http://localhost:8080".into()));
    ApiConfig::from_env(&Env::from_pairs(values.clone())).unwrap();
    WorkerConfig::from_env(&Env::from_pairs(values), &[]).unwrap();
    let mut valid = pairs();
    valid.push((
        "AWS_SECRET_ACCESS_KEY",
        uuid::Uuid::new_v4().simple().to_string(),
    ));
    valid.push((
        "AWS_SESSION_TOKEN",
        uuid::Uuid::new_v4().simple().to_string(),
    ));
    valid.push((
        "ARIAD_TURNSTILE_SECRET",
        uuid::Uuid::new_v4().simple().to_string(),
    ));
    let env = Env::from_pairs(valid);
    ApiConfig::from_env(&env).unwrap();
    WorkerConfig::from_env(&env, &[]).unwrap();
}
