//! Pure `serde` + `schemars` parameter structs for every registered document tool.
//!
//! All tool parameter types live in `ariad-core` with no engine or `lopdf` dependencies so
//! [`crate::tools::registry`] can resolve `params_schema` without crate cycles.

pub mod info;

use schemars::{JsonSchema, Schema, generate::SchemaSettings};

pub use info::InfoParams;

/// Generates a Draft 2020-12 JSON Schema for the parameter struct `P`.
#[must_use]
pub fn schema_for_params<P: JsonSchema>() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<P>()
}
