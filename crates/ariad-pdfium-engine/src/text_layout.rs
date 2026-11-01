//! The `pdf.text_layout` tool.

use ariad_core::tools::params::TextLayoutParams;

use crate::{
    not_implemented,
    output::{ToolContext, ToolResult},
};

/// Runs `pdf.text_layout`: emits its artifacts through `ctx` and returns the metrics of the result event.
pub fn run(_ctx: &mut ToolContext<'_>, _params: TextLayoutParams) -> ToolResult {
    Err(not_implemented("pdf.text_layout"))
}
