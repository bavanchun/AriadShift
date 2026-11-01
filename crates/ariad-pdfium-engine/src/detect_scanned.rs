//! The `pdf.detect_scanned` tool.

use ariad_core::tools::params::DetectScannedParams;

use crate::{
    not_implemented,
    output::{ToolContext, ToolResult},
};

/// Runs `pdf.detect_scanned`: emits its artifacts through `ctx` and returns the metrics of the result event.
pub fn run(_ctx: &mut ToolContext<'_>, _params: DetectScannedParams) -> ToolResult {
    Err(not_implemented("pdf.detect_scanned"))
}
