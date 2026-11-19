//! The integration tests of `ariad-server`, linked as one test binary.
//!
//! Each former test target is a module here; run one with, for example,
//! `cargo nextest run -p ariad-server --test it -E 'test(/^jobs::/)'` (anchored: a plain `jobs::`
//! substring would also select `cloud_tool_jobs::`).

mod auth;
mod auth_failure_isolation;
mod blob_download_admission;
mod blob_download_headers;
mod blob_transfers;
mod cloud_tool_jobs;
mod common;
mod digest_admission;
mod global_job_admission;
mod inline_storage;
mod isolated;
mod job_list;
mod jobs;
mod limits;
mod mcp_inline_failure;
mod mcp_remote;
mod openapi_drift;
mod openapi_v1_snapshot;
mod plan_limits;
mod queue_fairness;
mod s3_storage;
mod schema;
mod startup_secret_policy;
mod upload_deletion;
mod upload_verification;
mod worker_concurrency_policy;
mod worker_credentials;
