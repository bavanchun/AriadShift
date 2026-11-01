//! The `pdf.crop_whitespace` tool.

use ariad_core::tools::params::CropWhitespaceParams;

use crate::{
    not_implemented,
    output::{ToolContext, ToolResult},
};

/// Runs `pdf.crop_whitespace`: emits its artifacts through `ctx` and returns the metrics of the result event.
pub fn run(_ctx: &mut ToolContext<'_>, _params: CropWhitespaceParams) -> ToolResult {
    Err(not_implemented("pdf.crop_whitespace"))
}
