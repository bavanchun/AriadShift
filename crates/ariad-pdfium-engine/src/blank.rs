//! The `pdf.remove_blank` tool and the pixel `blank-separator` mode of `pdf.split`.

use ariad_core::tools::params::RemoveBlankParams;
use serde_json::Value;

use crate::{
    not_implemented,
    output::{ToolContext, ToolResult},
};

/// Runs `pdf.remove_blank`: emits its artifacts through `ctx` and returns the metrics of the
/// result event.
pub fn run(_ctx: &mut ToolContext<'_>, _params: RemoveBlankParams) -> ToolResult {
    Err(not_implemented("pdf.remove_blank"))
}

/// Runs `pdf.split` with `detect = "pixel"`.
///
/// `params` is the raw `pdf.split` parameter object because its typed form belongs to the
/// structural PDF tools; the body parses it there.
pub fn split_pixel(_ctx: &mut ToolContext<'_>, _params: &Value) -> ToolResult {
    Err(not_implemented("pdf.split"))
}
