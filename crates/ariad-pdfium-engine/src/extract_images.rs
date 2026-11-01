//! The `pdf.extract_images` tool.

use ariad_core::tools::params::ExtractImagesParams;

use crate::{
    not_implemented,
    output::{ToolContext, ToolResult},
};

/// Runs `pdf.extract_images`: emits its artifacts through `ctx` and returns the metrics of the result event.
pub fn run(_ctx: &mut ToolContext<'_>, _params: ExtractImagesParams) -> ToolResult {
    Err(not_implemented("pdf.extract_images"))
}
