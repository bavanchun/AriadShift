//! Typed messages for the draft AriadShift engine protocol.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::limits::Limits;

/// The protocol version implemented by this workspace.
pub const PROTOCOL: &str = "ariad-engine/1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Request {
    Convert {
        protocol: String,
        job: String,
        input: Input,
        output: Output,
        work_dir: String,
        #[serde(default)]
        options: BTreeMap<String, Value>,
        #[serde(default)]
        limits: Limits,
    },
    Describe {
        protocol: String,
        job: String,
    },
}

impl Request {
    /// Returns the protocol version string declared by this request.
    #[must_use]
    pub fn protocol(&self) -> &str {
        match self {
            Self::Convert { protocol, .. } | Self::Describe { protocol, .. } => protocol,
        }
    }

    /// Returns the job identifier declared by this request.
    #[must_use]
    pub fn job(&self) -> &str {
        match self {
            Self::Convert { job, .. } | Self::Describe { job, .. } => job,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Input {
    pub path: String,
    pub format: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Output {
    pub dir: String,
    pub format: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedRoute,
    LimitExceeded,
    EngineFailure,
    ToolMissing,
    ToolVersion,
    Io,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineError {
    pub code: ErrorCode,
    pub message: String,
}

/// Either direction of one JSONL protocol message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Message {
    Request(Box<Request>),
    Event(Box<Event>),
}

/// A single NDJSON event emitted by an engine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{EngineError, ErrorCode, Event, Input, Message, Output, PROTOCOL, Request};
    use crate::limits::Limits;

    #[test]
    fn reads_the_architecture_request_example_and_omits_unlimited_caps_when_written() {
        let example = r#"{"protocol":"ariad-engine/1","job":"job_01JABC","op":"convert","input":{"path":"/work/in/document.ir.json","format":"ariad-ir+json"},"output":{"dir":"/work/out","format":"docx"},"work_dir":"/work/tmp","options":{},"limits":{"max_pages":null,"timeout_s":null,"max_memory_mb":null,"max_nesting_depth":64}}"#;
        let request: Request = serde_json::from_str(example).expect("request example deserializes");
        let message: Message = serde_json::from_str(example).expect("request message deserializes");

        match &request {
            Request::Convert {
                protocol,
                job,
                input,
                output,
                limits,
                ..
            } => {
                assert_eq!(protocol, PROTOCOL);
                assert_eq!(job, "job_01JABC");
                assert_eq!(input.path, "/work/in/document.ir.json");
                assert_eq!(output.format, "docx");
                assert_eq!(limits, &Limits::local());
            }
            Request::Describe { .. } => panic!("expected convert request"),
        }
        assert!(matches!(message, Message::Request(_)));

        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["op"], "convert");
        assert!(encoded["limits"].get("timeout_s").is_none());
        assert_eq!(encoded["limits"]["max_nesting_depth"], 64);
    }

    #[test]
    fn describe_request_round_trips() {
        let describe = Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: "job-desc-1".to_owned(),
        };
        let value = serde_json::to_value(&describe).unwrap();
        assert_eq!(value["op"], "describe");
        assert_eq!(value["protocol"], PROTOCOL);
        assert_eq!(value["job"], "job-desc-1");
        assert!(value.get("input").is_none());
        assert!(value.get("output").is_none());
        assert!(value.get("work_dir").is_none());

        let round_trip: Request = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip, describe);
    }

    #[test]
    fn event_examples_are_tagged_and_optional_result_fields_are_omitted() {
        let examples = [
            json!({"type":"progress","stage":"layout","done":12,"total":37}),
            json!({"type":"warning","code":"font_missing","message":"A font was substituted"}),
            json!({"type":"artifact","path":"/work/out/document.docx","format":"docx"}),
            json!({"type":"result","ok":true,"metrics":{"pages":37,"elapsed_ms":8421}}),
        ];

        for example in examples {
            let event: Event = serde_json::from_value(example.clone()).expect("event deserializes");
            let message: Message =
                serde_json::from_value(example.clone()).expect("message deserializes");
            let encoded = serde_json::to_value(event).expect("event serializes");
            assert_eq!(encoded, example);
            assert!(matches!(message, Message::Event(_)));
        }

        let error_event = Event::Result {
            ok: false,
            metrics: None,
            error: Some(EngineError {
                code: ErrorCode::ToolMissing,
                message: "Install Pandoc with `just pandoc`.".to_owned(),
            }),
        };
        assert_eq!(
            serde_json::to_value(error_event).unwrap(),
            json!({"type":"result","ok":false,"error":{"code":"tool_missing","message":"Install Pandoc with `just pandoc`."}})
        );
    }

    #[test]
    fn request_accepts_arbitrary_options_as_a_json_map() {
        let request = Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: "job-1".to_owned(),
            input: Input {
                path: "/work/in/input.json".to_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            output: Output {
                dir: "/work/out".to_owned(),
                format: "docx".to_owned(),
            },
            work_dir: "/work/tmp".to_owned(),
            options: BTreeMap::from([("profile".to_owned(), Value::String("editable".to_owned()))]),
            limits: Limits::local(),
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["op"], "convert");
        assert_eq!(value["options"]["profile"], "editable");
    }

    #[test]
    fn error_codes_are_closed() {
        assert!(serde_json::from_str::<ErrorCode>("\"unknown\"").is_err());
    }
}
