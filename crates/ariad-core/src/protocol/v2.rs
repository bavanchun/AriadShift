//! Typed messages for the `ariad-engine/2` protocol.
//!
//! `ariad-engine/2` extends `ariad-engine/1` with `op = "run_tool"` and `capabilities.tools[]`
//! while keeping `/1` frozen and unchanged.

use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::limits::Limits;

use super::{
    EngineError, Event, Input, Output, PROTOCOL as PROTOCOL_V1, Request, RouteCapability,
    ToolStatus,
};

/// The protocol version string for `ariad-engine/2`.
pub const PROTOCOL: &str = "ariad-engine/2";

/// Alias for [`PROTOCOL`] when imported alongside `ariad-engine/1`.
pub const PROTOCOL_V2: &str = PROTOCOL;

/// Maximum serialized byte size of inline `params` in a `run_tool` request (256 KiB).
///
/// Larger parameter objects and secret parameters must be passed via `params_file`.
pub const MAX_INLINE_PARAMS_BYTES: usize = 256 * 1024;

fn default_params() -> Value {
    Value::Object(Map::new())
}

fn default_capabilities_protocol() -> String {
    PROTOCOL_V1.to_owned()
}

/// Target output directory for a tool execution in `ariad-engine/2`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OutputDir {
    pub dir: String,
}

/// A supported document tool reported by an `ariad-engine/2` engine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolCapability {
    pub id: String,
    pub version: String,
}

/// Validation failure for an `ariad-engine/2` request.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProtocolV2Error {
    #[error("run_tool request requires at least one input")]
    EmptyInputs,
    #[error("inline params ({actual} bytes) exceed the {max}-byte limit; use params_file")]
    InlineParamsTooLarge { actual: usize, max: usize },
}

/// An `ariad-engine/2` request sent on stdin as a single NDJSON line.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", remote = "Self")]
#[allow(clippy::large_enum_variant)]
pub enum RequestV2 {
    RunTool {
        protocol: String,
        job: String,
        tool: String,
        #[schemars(length(min = 1))]
        inputs: Vec<Input>,
        output: OutputDir,
        #[serde(default = "default_params")]
        params: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params_file: Option<PathBuf>,
        work_dir: String,
        limits: Limits,
    },
    Convert {
        protocol: String,
        job: String,
        input: Input,
        output: Output,
        work_dir: String,
        #[serde(default)]
        options: BTreeMap<String, Value>,
        limits: Limits,
        /// Warm session this job belongs to. An engine that reports `supports_session: true`
        /// keeps running after the `result` and reads the next request of the same session.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Describe {
        protocol: String,
        job: String,
    },
}

impl Serialize for RequestV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for RequestV2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let request = Self::deserialize(deserializer)?;
        request.validate().map_err(serde::de::Error::custom)?;
        Ok(request)
    }
}

impl RequestV2 {
    /// Validates structural invariants of an `ariad-engine/2` request:
    /// - `run_tool` must have at least one input (`!inputs.is_empty()`).
    /// - `run_tool` inline `params` must serialize to at most [`MAX_INLINE_PARAMS_BYTES`]
    ///   (256 KiB) unless `params_file` is provided.
    pub fn validate(&self) -> Result<(), ProtocolV2Error> {
        if let Self::RunTool {
            inputs,
            params,
            params_file,
            ..
        } = self
        {
            if inputs.is_empty() {
                return Err(ProtocolV2Error::EmptyInputs);
            }
            if params_file.is_none() {
                let actual = serde_json::to_vec(params).map_or(usize::MAX, |bytes| bytes.len());
                if actual > MAX_INLINE_PARAMS_BYTES {
                    return Err(ProtocolV2Error::InlineParamsTooLarge {
                        actual,
                        max: MAX_INLINE_PARAMS_BYTES,
                    });
                }
            }
        }
        Ok(())
    }

    /// Returns the protocol version string declared by this request.
    #[must_use]
    pub fn protocol(&self) -> &str {
        match self {
            Self::RunTool { protocol, .. }
            | Self::Convert { protocol, .. }
            | Self::Describe { protocol, .. } => protocol,
        }
    }

    /// Returns the job identifier declared by this request.
    #[must_use]
    pub fn job(&self) -> &str {
        match self {
            Self::RunTool { job, .. } | Self::Convert { job, .. } | Self::Describe { job, .. } => {
                job
            }
        }
    }

    /// Returns the session identifier of a `convert` request, if it belongs to a session.
    ///
    /// `run_tool` and `describe` requests never belong to a warm session (`None`).
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Convert { session_id, .. } => session_id.as_deref(),
            Self::RunTool { .. } | Self::Describe { .. } => None,
        }
    }

    /// Returns the working directory path if the request carries one (`run_tool` and `convert`).
    #[must_use]
    pub fn work_dir(&self) -> Option<&str> {
        match self {
            Self::RunTool { work_dir, .. } | Self::Convert { work_dir, .. } => Some(work_dir),
            Self::Describe { .. } => None,
        }
    }

    /// Returns the resource limits if the request carries them (`run_tool` and `convert`).
    #[must_use]
    pub fn limits(&self) -> Option<&Limits> {
        match self {
            Self::RunTool { limits, .. } | Self::Convert { limits, .. } => Some(limits),
            Self::Describe { .. } => None,
        }
    }

    /// Returns the output directory path if the request carries one (`run_tool` and `convert`).
    #[must_use]
    pub fn output_dir(&self) -> Option<&str> {
        match self {
            Self::RunTool { output, .. } => Some(&output.dir),
            Self::Convert { output, .. } => Some(&output.dir),
            Self::Describe { .. } => None,
        }
    }
}

impl From<Request> for RequestV2 {
    fn from(request: Request) -> Self {
        match request {
            Request::Convert {
                protocol,
                job,
                input,
                output,
                work_dir,
                options,
                limits,
                session_id,
            } => Self::Convert {
                protocol,
                job,
                input,
                output,
                work_dir,
                options,
                limits,
                session_id,
            },
            Request::Describe { protocol, job } => Self::Describe { protocol, job },
        }
    }
}

/// A single NDJSON event emitted by an `ariad-engine/2` engine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventV2 {
    Capabilities {
        /// Protocol version advertised by the engine; defaults to `"ariad-engine/1"` when
        /// reading `/1` capability events.
        #[serde(default = "default_capabilities_protocol")]
        protocol: String,
        engine: String,
        version: String,
        tool: ToolStatus,
        license: String,
        routes: Vec<RouteCapability>,
        /// Supported document tools advertised by an `ariad-engine/2` engine.
        #[serde(default)]
        tools: Vec<ToolCapability>,
        enforces_memory_limit: bool,
        /// Whether the engine serves several `convert` jobs of one `session_id` in one process.
        #[serde(default)]
        supports_session: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        models: Option<Vec<String>>,
    },
    Progress {
        stage: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        done: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    Warning {
        code: String,
        message: String,
    },
    Artifact {
        path: String,
        format: String,
    },
    Result {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        metrics: Option<BTreeMap<String, Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<EngineError>,
    },
}

impl From<Event> for EventV2 {
    fn from(event: Event) -> Self {
        match event {
            Event::Capabilities {
                engine,
                version,
                tool,
                license,
                routes,
                enforces_memory_limit,
                supports_session,
                models,
            } => Self::Capabilities {
                protocol: PROTOCOL_V1.to_owned(),
                engine,
                version,
                tool,
                license,
                routes,
                tools: Vec::new(),
                enforces_memory_limit,
                supports_session,
                models,
            },
            Event::Progress { stage, done, total } => Self::Progress { stage, done, total },
            Event::Warning { code, message } => Self::Warning { code, message },
            Event::Artifact { path, format } => Self::Artifact { path, format },
            Event::Result { ok, metrics, error } => Self::Result { ok, metrics, error },
        }
    }
}

impl From<EventV2> for Event {
    fn from(event: EventV2) -> Self {
        match event {
            EventV2::Capabilities {
                engine,
                version,
                tool,
                license,
                routes,
                enforces_memory_limit,
                supports_session,
                models,
                ..
            } => Self::Capabilities {
                engine,
                version,
                tool,
                license,
                routes,
                enforces_memory_limit,
                supports_session,
                models,
            },
            EventV2::Progress { stage, done, total } => Self::Progress { stage, done, total },
            EventV2::Warning { code, message } => Self::Warning { code, message },
            EventV2::Artifact { path, format } => Self::Artifact { path, format },
            EventV2::Result { ok, metrics, error } => Self::Result { ok, metrics, error },
        }
    }
}

/// Either direction of one JSONL `ariad-engine/2` protocol message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MessageV2 {
    Request(Box<RequestV2>),
    Event(Box<EventV2>),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use super::{
        EventV2, MAX_INLINE_PARAMS_BYTES, MessageV2, OutputDir, PROTOCOL, ProtocolV2Error,
        RequestV2, ToolCapability,
    };
    use crate::{
        limits::Limits,
        protocol::{Input, PROTOCOL as PROTOCOL_V1, ToolAvailability, ToolStatus},
    };

    #[test]
    fn canonical_run_tool_request_round_trips() {
        let example = r#"{"protocol":"ariad-engine/2","job":"job_01KXYZ","op":"run_tool","tool":"pdf.rotate","inputs":[{"path":"/work/in/0.pdf","format":"pdf"}],"output":{"dir":"/work/out"},"params":{"pages":"1-3,5","angle":90},"work_dir":"/work/tmp","limits":{"max_pages":null,"timeout_s":null,"max_memory_mb":null,"max_nesting_depth":64}}"#;
        let request: RequestV2 =
            serde_json::from_str(example).expect("run_tool request deserializes");
        let message: MessageV2 =
            serde_json::from_str(example).expect("run_tool message deserializes");
        assert!(matches!(message, MessageV2::Request(_)));
        assert_eq!(request.protocol(), PROTOCOL);
        assert_eq!(request.job(), "job_01KXYZ");
        assert_eq!(request.session_id(), None);
        assert_eq!(request.work_dir(), Some("/work/tmp"));
        assert_eq!(request.output_dir(), Some("/work/out"));
        assert_eq!(request.limits(), Some(&Limits::local()));

        match &request {
            RequestV2::RunTool {
                tool,
                inputs,
                output,
                params,
                params_file,
                ..
            } => {
                assert_eq!(tool, "pdf.rotate");
                assert_eq!(inputs.len(), 1);
                assert_eq!(inputs[0].path, "/work/in/0.pdf");
                assert_eq!(inputs[0].format, "pdf");
                assert_eq!(output.dir, "/work/out");
                assert_eq!(params["angle"], 90);
                assert!(params_file.is_none());
            }
            _ => panic!("expected RunTool variant"),
        }

        let encoded = serde_json::to_value(&request).expect("run_tool serializes");
        assert_eq!(encoded["op"], "run_tool");
        assert_eq!(encoded["protocol"], PROTOCOL);
        assert!(encoded.get("params_file").is_none());
        let round_trip: RequestV2 = serde_json::from_value(encoded).expect("round-trips");
        assert_eq!(round_trip, request);
    }

    #[test]
    fn unknown_op_is_rejected() {
        let unknown = json!({
            "protocol": PROTOCOL,
            "job": "job-unknown",
            "op": "unknown_op",
            "inputs": [{"path": "/work/in/0.pdf", "format": "pdf"}],
            "output": {"dir": "/work/out"},
            "params": {},
            "work_dir": "/work/tmp",
            "limits": {"max_nesting_depth": 64}
        });
        assert!(serde_json::from_value::<RequestV2>(unknown.clone()).is_err());
        assert!(serde_json::from_value::<MessageV2>(unknown).is_err());
    }

    #[test]
    fn empty_inputs_are_rejected() {
        let empty_inputs = json!({
            "protocol": PROTOCOL,
            "job": "job-empty",
            "op": "run_tool",
            "tool": "pdf.merge",
            "inputs": [],
            "output": {"dir": "/work/out"},
            "params": {},
            "work_dir": "/work/tmp",
            "limits": {"max_nesting_depth": 64}
        });
        assert!(serde_json::from_value::<RequestV2>(empty_inputs.clone()).is_err());
        assert!(serde_json::from_value::<MessageV2>(empty_inputs).is_err());

        let constructed = RequestV2::RunTool {
            protocol: PROTOCOL.to_owned(),
            job: "job-empty".to_owned(),
            tool: "pdf.merge".to_owned(),
            inputs: vec![],
            output: OutputDir {
                dir: "/work/out".to_owned(),
            },
            params: json!({}),
            params_file: None,
            work_dir: "/work/tmp".to_owned(),
            limits: Limits::local(),
        };
        assert_eq!(constructed.validate(), Err(ProtocolV2Error::EmptyInputs));
    }

    #[test]
    fn inline_params_over_256_kib_rejected_unless_params_file_is_present() {
        let oversized_blob = "x".repeat(MAX_INLINE_PARAMS_BYTES + 16);
        let inline_oversized = json!({
            "protocol": PROTOCOL,
            "job": "job-big-params",
            "op": "run_tool",
            "tool": "pdf.outline",
            "inputs": [{"path": "/work/in/0.pdf", "format": "pdf"}],
            "output": {"dir": "/work/out"},
            "params": {"bookmark_tree": oversized_blob},
            "work_dir": "/work/tmp",
            "limits": {"max_nesting_depth": 64}
        });
        assert!(serde_json::from_value::<RequestV2>(inline_oversized.clone()).is_err());
        assert!(serde_json::from_value::<MessageV2>(inline_oversized).is_err());

        let mut with_params_file = json!({
            "protocol": PROTOCOL,
            "job": "job-big-params-file",
            "op": "run_tool",
            "tool": "pdf.outline",
            "inputs": [{"path": "/work/in/0.pdf", "format": "pdf"}],
            "output": {"dir": "/work/out"},
            "params": {},
            "params_file": "/work/tmp/params.json",
            "work_dir": "/work/tmp",
            "limits": {"max_nesting_depth": 64}
        });
        let parsed: RequestV2 =
            serde_json::from_value(with_params_file.clone()).expect("accepted with params_file");
        assert!(parsed.validate().is_ok());

        // Even if the in-memory struct carries oversized params, setting params_file satisfies
        // the "unless params_file" rule.
        with_params_file["params"] = json!({"bookmark_tree": oversized_blob});
        let parsed_with_both: RequestV2 = serde_json::from_value(with_params_file)
            .expect("accepted when params_file is specified");
        assert!(parsed_with_both.validate().is_ok());

        // Exact boundary check: <= 256 KiB inline is accepted.
        let boundary_str = "a".repeat(MAX_INLINE_PARAMS_BYTES - 16);
        let boundary_req = RequestV2::RunTool {
            protocol: PROTOCOL.to_owned(),
            job: "job-boundary".to_owned(),
            tool: "pdf.outline".to_owned(),
            inputs: vec![Input {
                path: "/work/in/0.pdf".to_owned(),
                format: "pdf".to_owned(),
            }],
            output: OutputDir {
                dir: "/work/out".to_owned(),
            },
            params: Value::String(boundary_str),
            params_file: None,
            work_dir: "/work/tmp".to_owned(),
            limits: Limits::local(),
        };
        assert!(boundary_req.validate().is_ok());
        let boundary_json = serde_json::to_string(&boundary_req).unwrap();
        let boundary_parsed: RequestV2 = serde_json::from_str(&boundary_json).unwrap();
        assert_eq!(boundary_parsed, boundary_req);

        let over_req = RequestV2::RunTool {
            protocol: PROTOCOL.to_owned(),
            job: "job-over".to_owned(),
            tool: "pdf.outline".to_owned(),
            inputs: vec![Input {
                path: "/work/in/0.pdf".to_owned(),
                format: "pdf".to_owned(),
            }],
            output: OutputDir {
                dir: "/work/out".to_owned(),
            },
            params: Value::String("a".repeat(MAX_INLINE_PARAMS_BYTES)),
            params_file: Some(PathBuf::from("/work/tmp/params.json")),
            work_dir: "/work/tmp".to_owned(),
            limits: Limits::local(),
        };
        assert!(over_req.validate().is_ok());
    }

    #[test]
    fn capabilities_v2_defaults_protocol_to_v1_and_round_trips_tools() {
        let v1_cap = json!({
            "type": "capabilities",
            "engine": "pandoc",
            "version": "0.1.0",
            "tool": {"name": "pandoc", "version": "3.12", "status": "found"},
            "license": "GPL-2.0-or-later",
            "routes": [{"input": "ariad-ir+json", "output": "docx"}],
            "enforces_memory_limit": true,
            "supports_session": false
        });
        let parsed_v1: EventV2 = serde_json::from_value(v1_cap).expect("v1 capabilities parse");
        match parsed_v1 {
            EventV2::Capabilities {
                protocol, tools, ..
            } => {
                assert_eq!(protocol, PROTOCOL_V1);
                assert!(tools.is_empty());
            }
            _ => panic!("expected Capabilities"),
        }

        let v2_cap = EventV2::Capabilities {
            protocol: PROTOCOL.to_owned(),
            engine: "pdfium".to_owned(),
            version: "0.1.0".to_owned(),
            tool: ToolStatus {
                name: "pdfium".to_owned(),
                version: Some("158.0.8125.0".to_owned()),
                status: ToolAvailability::Found,
            },
            license: "BSD-3-Clause AND Apache-2.0".to_owned(),
            routes: vec![],
            tools: vec![ToolCapability {
                id: "pdf.rotate".to_owned(),
                version: "1".to_owned(),
            }],
            enforces_memory_limit: false,
            supports_session: false,
            models: None,
        };
        let encoded = serde_json::to_value(&v2_cap).unwrap();
        assert_eq!(encoded["protocol"], PROTOCOL);
        assert_eq!(
            encoded["tools"],
            json!([{"id": "pdf.rotate", "version": "1"}])
        );
        let round_trip: EventV2 = serde_json::from_value(encoded).unwrap();
        assert_eq!(round_trip, v2_cap);
    }
}
