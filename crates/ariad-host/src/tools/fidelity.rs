//! Shared measured fidelity attachment for document-producing tools.

use ariad_core::fidelity::FidelityReport;

use super::ToolOutcome;

/// Attach an actual conversion/read-back report while preserving other tool evidence.
/// The future document export tool calls this after its conversion has completed.
pub fn attach_document_fidelity(
    outcome: &mut ToolOutcome,
    report: &FidelityReport,
) -> Result<(), serde_json::Error> {
    outcome
        .metrics
        .insert("fidelity".into(), serde_json::to_value(report)?);
    Ok(())
}
