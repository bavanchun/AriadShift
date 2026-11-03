//! Parameter type for the `pdf.info` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `pdf.info`.
///
/// `pdf.info` takes no parameters (`{}`) and rejects any unknown fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {}
