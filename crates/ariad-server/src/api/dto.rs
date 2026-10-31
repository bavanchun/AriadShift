//! Request and response bodies of the `/v1` API.
//!
//! These types are the public contract and feed the OpenAPI document; they
//! are mapped from internal and `ariad-core` types instead of exposing them.

use std::collections::BTreeMap;

use ariad_core::{format::Format, planner};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    db::{JobStatus, UploadStatus},
    error::ErrorDetail,
    packs::Pack,
};

/// Route selection priorities.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Semantic accuracy: structure, headings, lists, tables.
    #[default]
    Editable,
    /// Visual and page layout preservation.
    Faithful,
    /// Execution speed.
    Fast,
}

impl From<Profile> for planner::Profile {
    fn from(profile: Profile) -> Self {
        match profile {
            Profile::Editable => Self::Editable,
            Profile::Faithful => Self::Faithful,
            Profile::Fast => Self::Fast,
        }
    }
}

impl Profile {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "editable" => Some(Self::Editable),
            "faithful" => Some(Self::Faithful),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Editable => "editable",
            Self::Faithful => "faithful",
            Self::Fast => "fast",
        }
    }
}

/// Raw file bytes, the body of a presigned object `PUT` or `GET`.
pub struct FileBytes;

impl utoipa::PartialSchema for FileBytes {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::Type::String)
            .format(Some(utoipa::openapi::SchemaFormat::KnownFormat(
                utoipa::openapi::KnownFormat::Binary,
            )))
            .into()
    }
}

impl ToSchema for FileBytes {}

/// Starts an upload.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUploadRequest {
    /// Original file name. Used for the output name and title fallback; never logged.
    #[schema(example = "report.docx", max_length = 255)]
    pub filename: String,
    /// Exact size in bytes.
    #[schema(example = 48213)]
    pub size: u64,
    /// SHA-256 of the file, lowercase hex.
    #[schema(example = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")]
    pub sha256: String,
}

/// The single request that transfers a file to or from storage.
#[derive(Debug, Serialize, ToSchema)]
pub struct PresignedRequest {
    /// HTTP method, `PUT` for uploads.
    #[schema(example = "PUT")]
    pub method: String,
    /// Presigned URL.
    pub url: String,
    /// Headers to send unchanged.
    pub headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Upload {
    pub id: Uuid,
    pub status: UploadStatus,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
    /// Detected format registry id, once the upload is complete.
    pub format: Option<String>,
    pub created_at: DateTime<Utc>,
    /// When the stored file is deleted.
    pub expires_at: DateTime<Utc>,
    /// Present while the upload is pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload: Option<PresignedRequest>,
}

/// Plans a route without converting.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    /// Source format id or extension (`markdown`, `md`, `docx`). Required unless `upload_id` is given.
    #[schema(example = "md")]
    pub from: Option<String>,
    /// A completed upload whose detected format is the source.
    pub upload_id: Option<Uuid>,
    /// Target format id or extension.
    #[schema(example = "docx")]
    pub to: String,
    #[serde(default)]
    pub profile: Profile,
}

/// One conversion step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub struct RouteStep {
    #[schema(example = "markdown")]
    pub from: String,
    #[schema(example = "ariad-ir+json")]
    pub to: String,
    #[schema(example = "ariad-core")]
    pub engine: String,
}

impl From<&planner::PlanStep> for RouteStep {
    fn from(step: &planner::PlanStep) -> Self {
        Self {
            from: step.from.id().to_owned(),
            to: step.to.id().to_owned(),
            engine: step.engine.clone(),
        }
    }
}

/// Benchmark-derived scores of a route; `null` where unmeasured.
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct RouteScore {
    pub fidelity: Option<f64>,
    pub editability: Option<f64>,
    pub estimated_duration_ms: Option<f64>,
}

impl From<&planner::PlanScore> for RouteScore {
    fn from(score: &planner::PlanScore) -> Self {
        Self {
            fidelity: score.fidelity,
            editability: score.editability,
            estimated_duration_ms: score.estimated_duration_ms,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanAlternative {
    pub route: Vec<RouteStep>,
    pub score: RouteScore,
    pub measured: bool,
    pub pack: Pack,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanResponse {
    pub from: String,
    pub to: String,
    pub profile: Profile,
    /// Worker pack that runs the route.
    pub pack: Pack,
    pub route: Vec<RouteStep>,
    pub score: RouteScore,
    /// Whether every step has benchmark measurements.
    pub measured: bool,
    pub alternatives: Vec<PlanAlternative>,
}

/// Creates a conversion job.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateJobRequest {
    /// A completed upload.
    pub upload_id: Uuid,
    /// Target format id or extension.
    #[schema(example = "docx")]
    pub to: String,
    #[serde(default)]
    pub profile: Profile,
}

/// A warning reported by the conversion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub struct JobWarning {
    #[schema(max_length = 64)]
    pub code: String,
    #[schema(max_length = 512)]
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Job {
    pub id: Uuid,
    pub status: JobStatus,
    pub upload_id: Uuid,
    pub from: String,
    pub to: String,
    pub profile: Profile,
    pub pack: Pack,
    /// Planned route; replaced by the executed route when the job succeeds.
    pub route: Vec<RouteStep>,
    #[schema(max_items = 64)]
    pub warnings: Vec<JobWarning>,
    /// Set when the job failed.
    pub error: Option<ErrorDetail>,
    /// Worker attempts so far.
    pub attempts: u32,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    /// When inputs and outputs are deleted.
    pub expires_at: DateTime<Utc>,
}

/// A produced file.
#[derive(Debug, Serialize, ToSchema)]
pub struct OutputFile {
    pub id: Uuid,
    #[schema(example = "report.docx")]
    pub filename: String,
    #[schema(example = "docx")]
    pub format: String,
    pub media_type: String,
    pub size: u64,
    pub sha256: String,
    /// Presigned download URL.
    pub url: String,
    /// The URL stops working at this time (five minutes after issue).
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Outputs {
    pub job_id: Uuid,
    pub outputs: Vec<OutputFile>,
}

/// One entry of a job's event stream (the `data` of an SSE message).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct JobEvent {
    /// Monotonic event id; also the SSE `id`, usable as `Last-Event-ID`.
    pub id: i64,
    pub job_id: Uuid,
    /// `queued`, `started`, `progress`, `warning`, `retrying`, `succeeded`, `failed`, `canceled` or `expired`.
    #[schema(example = "progress")]
    pub kind: String,
    /// Kind-specific details, for example `{"stage": "write"}`.
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// A format the service knows.
#[derive(Debug, Serialize, ToSchema)]
pub struct FormatInfo {
    #[schema(example = "markdown")]
    pub id: String,
    pub extensions: Vec<String>,
    pub media_type: String,
}

impl From<Format> for FormatInfo {
    fn from(format: Format) -> Self {
        Self {
            id: format.id().to_owned(),
            extensions: format
                .extensions()
                .iter()
                .map(|e| (*e).to_owned())
                .collect(),
            media_type: format.media_type().to_owned(),
        }
    }
}

/// Benchmark measurements of one conversion edge.
#[derive(Debug, Serialize, ToSchema)]
pub struct EdgeMetrics {
    pub fidelity: f64,
    pub editability: f64,
    pub p50_ms: f64,
    pub peak_mem_mb: f64,
    pub samples: u32,
}

/// One conversion edge an engine provides.
#[derive(Debug, Serialize, ToSchema)]
pub struct CapabilityEdge {
    pub from: String,
    pub to: String,
    pub engine: String,
    pub pack: Pack,
    pub license: String,
    /// `null` until the benchmark measures the edge.
    pub metrics: Option<EdgeMetrics>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Capabilities {
    /// Version of the capabilities registry, for example `ariad-capabilities/0`.
    pub version: String,
    pub generated_at: String,
    /// Packs with workers on this deployment.
    pub packs: Vec<Pack>,
    pub formats: Vec<FormatInfo>,
    /// Edges the deployment can run.
    pub edges: Vec<CapabilityEdge>,
}

/// An allowance of the caller's plan and how much of it is used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct Quota {
    /// The cap; `null` when the plan sets none.
    #[schema(required = true)]
    pub limit: Option<u64>,
    /// What currently counts against the cap.
    pub used: u64,
    /// What is left before requests are refused; `null` when unlimited.
    #[schema(required = true)]
    pub remaining: Option<u64>,
}

impl Quota {
    #[must_use]
    pub fn new(limit: Option<u64>, used: u64) -> Self {
        Self {
            limit,
            used,
            remaining: limit.map(|cap| cap.saturating_sub(used)),
        }
    }
}

/// Limits of every job of the plan; `null` means no limit.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConversionLimits {
    /// Largest accepted input file, in bytes.
    #[schema(required = true)]
    pub max_input_bytes: Option<u64>,
    /// Most pages a paged input may have.
    #[schema(required = true)]
    pub max_pages: Option<u32>,
    /// Longest processing time of one job, in seconds.
    #[schema(required = true)]
    pub timeout_s: Option<u64>,
    /// Memory budget of one job, in MiB.
    #[schema(required = true)]
    pub max_memory_mb: Option<u32>,
}

/// The caller's quotas. Daily quotas count the last 24 hours.
#[derive(Debug, Serialize, ToSchema)]
pub struct Quotas {
    /// Uploads started, including deleted and abandoned ones.
    pub uploads_per_day: Quota,
    /// Jobs created.
    pub jobs_per_day: Quota,
    /// The larger of the bytes uploaded and the input bytes of jobs created.
    pub input_bytes_per_day: Quota,
    /// Uploads waiting for their file and `complete` call.
    pub pending_uploads: Quota,
    /// Jobs queued or running.
    pub concurrent_jobs: Quota,
}

/// What the caller may convert, for showing limits before an upload.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlanLimits {
    /// Plan name, for example `anonymous` or `free`.
    pub plan: String,
    /// Whether the request carried valid credentials.
    pub authenticated: bool,
    /// How long inputs and outputs are kept, in seconds.
    pub retention_s: u64,
    /// Requests allowed per minute; `null` when unlimited.
    #[schema(required = true)]
    pub requests_per_minute: Option<u32>,
    pub limits: ConversionLimits,
    pub quotas: Quotas,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    #[schema(example = "ok")]
    pub status: String,
}
