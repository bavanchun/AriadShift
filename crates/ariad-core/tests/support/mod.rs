use std::path::PathBuf;

use schemars::generate::SchemaSettings;
use serde_json::json;

use ariad_core::{ir::Document, protocol::Message};

#[allow(dead_code)]
pub const IR_SCHEMA_ID: &str = "https://ariadshift.ariadnev.com/schemas/ir.v0.json";
pub const ENGINE_PROTOCOL_SCHEMA_ID: &str =
    "https://ariadshift.ariadnev.com/schemas/engine-protocol.v1.json";

#[allow(dead_code)]
#[path = "../../../ariad-cli/tests/support/fixtures.rs"]
pub mod fixtures;

pub fn repository_root() -> PathBuf {
    fixtures::repository_root()
}

pub fn ir_schema_path() -> PathBuf {
    repository_root().join("schemas/ir.v0.json")
}

#[allow(dead_code)]
pub fn engine_protocol_schema_path() -> PathBuf {
    repository_root().join("schemas/engine-protocol.v1.json")
}

#[allow(dead_code)]
pub fn generated_ir_schema() -> String {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<Document>();
    schema
        .as_object_mut()
        .expect("Document schema is an object")
        .insert("$id".to_owned(), json!(IR_SCHEMA_ID));
    let mut contents = serde_json::to_string_pretty(&schema).expect("schema serializes to JSON");
    contents.push('\n');
    contents
}

#[allow(dead_code)]
pub fn generated_engine_protocol_schema() -> String {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<Message>();
    let object = schema
        .as_object_mut()
        .expect("engine protocol schema is an object");
    object.insert("$id".to_owned(), json!(ENGINE_PROTOCOL_SCHEMA_ID));
    object.insert("x-status".to_owned(), json!("draft"));
    let mut contents = serde_json::to_string_pretty(&schema).expect("schema serializes to JSON");
    contents.push('\n');
    contents
}
