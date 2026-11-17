//! `pdf.tables_to_csv`: extracts the tables drawn with ruling lines into one CSV file each.
//!
//! This is the in-process extractor used where the Docling engine pack cannot run (the browser
//! and cloud runtimes) and as the fallback of the local runtime. Tables without ruling lines are
//! not detected.

use std::collections::BTreeMap;

use ariad_core::{
    reader::pdf::{self, ReadOptions},
    tools::{params::TablesToCsvParams, tables_csv},
};
use serde_json::Value;

use super::{InputSource, OutputArtifact, ToolOutput, ToolWarning};
use crate::{document::ToolCtx, error::PdfError, naming::single_output_name};

/// Runs `pdf.tables_to_csv` over exactly one raw PDF input.
pub(super) fn run(
    inputs: &[super::InputDoc],
    params: &Value,
    ctx: &ToolCtx,
) -> Result<ToolOutput, PdfError> {
    let _: TablesToCsvParams = serde_json::from_value(params.clone())
        .map_err(|error| PdfError::InvalidParams(error.to_string()))?;
    let [input] = inputs else {
        return Err(PdfError::InvalidParams(format!(
            "pdf.tables_to_csv requires exactly 1 input document, got {}",
            inputs.len()
        )));
    };
    let InputSource::Bytes(bytes) = &input.source else {
        return Err(PdfError::InvalidParams(
            "pdf.tables_to_csv requires raw PDF byte inputs".to_owned(),
        ));
    };

    ctx.check()?;
    let read = pdf::read_with_options(
        bytes,
        &input.limits,
        &ReadOptions {
            detect_tables: true,
        },
    )?;
    ctx.check()?;

    let extracted = tables_csv::extract(&read.document).map_err(|error| match error {
        tables_csv::TablesCsvError::NoTables => PdfError::NoTables,
        tables_csv::TablesCsvError::LimitExceeded => PdfError::TablesTooLarge,
    })?;

    let cap = extracted.cap_message().map(|message| ToolWarning {
        code: "unsupported_node".to_owned(),
        message,
    });
    let coded_warnings = read
        .warnings
        .iter()
        .chain(&extracted.warnings)
        .map(|warning| ToolWarning {
            code: serde_json::to_value(warning.code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "conversion_warning".to_owned()),
            message: warning.message.clone(),
        })
        .filter(|warning| tables_csv::keeps_read_warning_code(&warning.code))
        .chain(cap)
        .collect();

    let artifacts = extracted
        .tables
        .into_iter()
        .map(|table| OutputArtifact {
            name: single_output_name(&input.stem, &format!("table-{:03}", table.number), "csv"),
            format: "csv".to_owned(),
            bytes: table.csv.into_bytes(),
        })
        .collect();

    Ok(ToolOutput {
        artifacts,
        warnings: Vec::new(),
        coded_warnings,
        metrics: BTreeMap::new(),
        json: None,
    })
}
