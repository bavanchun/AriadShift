//! The `pdf.thumbnails` tool.

use ariad_core::tools::params::ThumbnailsParams;

use crate::{
    not_implemented,
    output::{ToolContext, ToolResult},
};

/// Runs `pdf.thumbnails`: emits its artifacts through `ctx` and returns the metrics of the result event.
pub fn run(_ctx: &mut ToolContext<'_>, _params: ThumbnailsParams) -> ToolResult {
    Err(not_implemented("pdf.thumbnails"))
}
