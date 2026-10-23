use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Default, Deserialize, Serialize, TS)]
pub struct Empty {}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, TS, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    #[default]
    Editable,
    Faithful,
    Fast,
    Private,
}
impl From<Profile> for ariad_host::convert::Profile {
    fn from(value: Profile) -> Self {
        match value {
            Profile::Editable => Self::Editable,
            Profile::Faithful => Self::Faithful,
            Profile::Fast => Self::Fast,
            Profile::Private => Self::Private,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, TS, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OutputPolicy {
    #[default]
    Adjacent,
    Folder,
}
#[derive(Clone, Deserialize, Serialize, TS)]
#[serde(default)]
pub struct Settings {
    pub profile: Profile,
    pub output_policy: OutputPolicy,
    pub output_folder: Option<String>,
    pub overwrite: bool,
    pub telemetry: bool,
    pub concurrency: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            profile: Profile::Editable,
            output_policy: OutputPolicy::Adjacent,
            output_folder: None,
            overwrite: false,
            telemetry: false,
            concurrency: 2,
        }
    }
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct QueueRequest {
    pub paths: Vec<String>,
    pub target: String,
    pub profile: Profile,
    pub output_folder: Option<String>,
    pub overwrite: bool,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct IdRequest {
    pub id: String,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct InspectRequest {
    pub path: String,
    pub target: String,
    pub profile: Profile,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct Inspection {
    pub summary: String,
    pub targets: Vec<String>,
    pub explanation: String,
    pub warnings: Vec<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct JobView {
    pub id: String,
    pub input: String,
    pub output: String,
    pub target: String,
    pub profile: Profile,
    pub status: JobStatus,
    pub stage: String,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct WatchRequest {
    #[serde(default = "default_poll_interval")]
    pub interval_seconds: u32,
    #[serde(default)]
    pub recursive: bool,
    pub input: String,
    pub output: String,
    pub from: String,
    pub target: String,
    pub profile: Profile,
    pub overwrite: bool,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct WatchView {
    pub id: String,
    pub route: WatchRequest,
    pub paused: bool,
    pub error: Option<String>,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct WatchControl {
    pub id: String,
    pub paused: bool,
    pub remove: bool,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct PackRow {
    pub name: String,
    pub version: String,
    pub description: String,
    pub installed: bool,
    pub active: bool,
}
#[derive(Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum PackAction {
    Install,
    Verify,
    Remove,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct PackRequest {
    pub name: String,
    pub version: Option<String>,
    pub action: PackAction,
    pub from: Option<String>,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct Diagnostic {
    pub id: String,
    pub ok: bool,
    pub required: bool,
    pub message: String,
    pub hint: String,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct Snapshot {
    pub jobs: Vec<JobView>,
    pub watches: Vec<WatchView>,
    pub settings: Settings,
    pub pack_progress: String,
    pub pack_busy: bool,
}
#[derive(Clone, Deserialize, Serialize, TS)]
pub struct DeepLink {
    pub from: String,
    pub target: String,
    pub profile: Profile,
}

#[derive(Clone, Deserialize, Serialize, TS)]
pub struct LinkRequest {
    pub url: String,
}

fn default_poll_interval() -> u32 {
    2
}
