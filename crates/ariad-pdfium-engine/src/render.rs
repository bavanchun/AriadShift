//! The `pdf.render` tool.

use ariad_core::tools::params::RenderParams;

use crate::{
    not_implemented,
    output::{ToolContext, ToolResult},
};

/// Runs `pdf.render`: emits its artifacts through `ctx` and returns the metrics of the result event.
pub fn run(_ctx: &mut ToolContext<'_>, _params: RenderParams) -> ToolResult {
    Err(not_implemented("pdf.render"))
}
