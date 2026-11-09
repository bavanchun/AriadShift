//! Tool specification for table extraction to CSV.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{schema_for_params, tables_to_csv::TablesToCsvParams},
};

/// Artifact formats of `pdf.tables_to_csv`: one CSV file per table.
pub const TABLES_TO_CSV_OUTPUT_FORMATS: &[&str] = &["csv"];

const LOCAL: &[ToolRuntime] = &[ToolRuntime::Local];

/// Specification for `pdf.tables_to_csv`.
pub const PDF_TABLES_TO_CSV_SPEC: ToolSpec = ToolSpec {
    id: "pdf.tables_to_csv",
    version: "1",
    group: ToolGroup::Convert,
    summary: "Extract tables from a PDF document to individual CSV files.",
    inputs: InputArity::One,
    input_formats: &["pdf"],
    output_format: "csv",
    output_formats: TABLES_TO_CSV_OUTPUT_FORMATS,
    tier: Tier::Free,
    runtimes: LOCAL,
    engine: "ariad-host",
    params_schema: schema_for_params::<TablesToCsvParams>,
    secret_params: &[],
};

/// Returns tool specifications exported by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![PDF_TABLES_TO_CSV_SPEC]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_integrity() {
        assert_eq!(PDF_TABLES_TO_CSV_SPEC.id, "pdf.tables_to_csv");
        assert_eq!(PDF_TABLES_TO_CSV_SPEC.group, ToolGroup::Convert);
        assert_eq!(PDF_TABLES_TO_CSV_SPEC.output_format, "csv");
    }
}
