//! Host-side execution for the `pdf.tables_to_csv` tool.
//!
//! Extracts tables from an input PDF document, serializing each discovered table into a
//! separate CSV file with formula-injection guarding. The Docling engine pack extracts the
//! tables when it is installed; otherwise the built-in PDF reader detects tables drawn with
//! ruling lines and tables whose columns are aligned with whitespace.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ariad_core::{
    ir::Document,
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
    tools::{
        ToolSpec,
        tables_csv::{self, TablesCsvError},
    },
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::{
    inprocess::{self, RawToolExecution},
    run::{ToolError, ToolEvent},
    secrets::SecretScrubber,
};
use crate::{
    commands::inspect::PdfSummary,
    convert::{ConvertError, ConvertWarning},
    engines::{self, EngineInvocation},
    workspace::Workspace,
};

/// Maximum number of tables extracted into CSV files by `pdf.tables_to_csv`.
pub use tables_csv::MAX_TABLES_COUNT;
/// Maximum cumulative CSV bytes emitted by `pdf.tables_to_csv` (50 MiB).
pub use tables_csv::MAX_TOTAL_CSV_BYTES;

/// Checks engine prerequisites before staging inputs or executing.
///
/// Docling extracts the tables when its pack is installed; otherwise the built-in reader runs in
/// the PDF engine child process, which must be resolvable.
pub fn preflight(engine_program: Option<&Path>) -> Result<(), ToolError> {
    if docling_installed(engine_program)
        || engines::resolve_engine_program(engine_program).is_some()
    {
        return Ok(());
    }
    Err(ToolError::from_missing_engine(engines::PDF.id))
}

fn docling_installed(engine_program: Option<&Path>) -> bool {
    engines::resolve_pack_with_options(engines::docling::ENGINE_ID, engine_program).is_ok()
}

/// The engine that reads the tables of one run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TableReader {
    /// The Docling pack, installed and preferred.
    Docling,
    /// The built-in reader in the PDF engine child process: ruled and whitespace-aligned tables.
    BuiltIn,
}

impl TableReader {
    /// Picks the reader once per run, so the run and the engine it reports agree.
    pub(super) fn detect(engine_program: Option<&Path>) -> Self {
        if docling_installed(engine_program) {
            Self::Docling
        } else {
            Self::BuiltIn
        }
    }

    /// Identifier of the engine that actually reads the tables; `built_in` is the tool's own.
    pub(super) fn engine_id(self, built_in: &str) -> String {
        match self {
            Self::Docling => engines::docling::ENGINE_ID.to_owned(),
            Self::BuiltIn => built_in.to_owned(),
        }
    }
}

/// Output artifact tuple and warnings produced by table extraction.
pub type TablesArtifacts = (Vec<(PathBuf, String)>, Vec<ConvertWarning>);

/// Converts tables found in an IR document into CSV artifact files inside the workspace.
pub fn tables_to_csv_artifacts(
    document: &Document,
    workspace: &Workspace,
) -> Result<TablesArtifacts, ToolError> {
    stage_csv_artifacts(document, workspace, &SecretScrubber::new())
}

fn stage_csv_artifacts(
    document: &Document,
    workspace: &Workspace,
    scrubber: &SecretScrubber,
) -> Result<TablesArtifacts, ToolError> {
    let extracted = tables_csv::extract(document).map_err(|error| match error {
        TablesCsvError::NoTables => ToolError::EngineFailure(tables_csv::NO_TABLES_MESSAGE.into()),
        TablesCsvError::LimitExceeded => ToolError::LimitExceeded,
    })?;
    let warnings = extracted
        .cap_message()
        .map(|message| ConvertWarning {
            code: "unsupported_node".to_owned(),
            message,
        })
        .into_iter()
        .chain(extracted.warnings.iter().map(crate::convert::safe_warning))
        .collect();
    let mut artifacts = Vec::with_capacity(extracted.tables.len());
    for table in &extracted.tables {
        let path = super::run::write_staged_artifact(
            workspace,
            &tables_csv::csv_file_name(table.number),
            table.csv.as_bytes(),
            scrubber,
        )?;
        artifacts.push((path, "csv".to_owned()));
    }
    Ok((artifacts, warnings))
}

/// Reads the PDF with the built-in reader, which detects tables drawn with ruling lines and tables
/// whose columns are aligned with whitespace.
///
/// The reader runs in the PDF engine child process, like the conversion routes. A PDF without
/// any text layer is a scan that only Docling can read, so it reports the missing pack.
fn extract_document_in_process(
    input_path: &Path,
    limits: &Limits,
    engine_program: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<(Document, Vec<ConvertWarning>), ToolError> {
    let program = engines::resolve_engine_program(engine_program)
        .ok_or_else(|| ToolError::from_missing_engine(engines::PDF.id))?;
    let mut workspace = Workspace::new().map_err(|e| ToolError::OutputIo(e.to_string()))?;
    let options = BTreeMap::from([(
        engines::pdf::DETECT_TABLES_OPTION.to_owned(),
        Value::Bool(true),
    )]);
    let read = crate::convert::read_to_ir_with_engine_options(
        input_path,
        ariad_core::format::Format::Pdf,
        engines::PDF.id,
        &mut workspace,
        limits,
        &EngineInvocation::new(&program),
        options,
        cancel.clone(),
    );
    let _ = workspace.close();
    let read = read.map_err(map_read_error)?;
    if PdfSummary::from_document(&read.document)
        .is_some_and(|summary| summary.needs_ocr && summary.text_pages == 0)
    {
        return Err(ToolError::from_missing_engine(
            crate::engines::docling::ENGINE_ID,
        ));
    }
    let warnings = read
        .warnings
        .into_iter()
        .filter(|warning| tables_csv::keeps_read_warning_code(&warning.code))
        .collect();
    Ok((read.document, warnings))
}

fn map_read_error(error: ConvertError) -> ToolError {
    match error {
        ConvertError::Interrupted => ToolError::Interrupted,
        ConvertError::LimitExceeded | ConvertError::MemoryLimitExceeded { .. } => {
            ToolError::LimitExceeded
        }
        ConvertError::EngineMissing { engine, .. } => ToolError::from_missing_engine(&engine),
        ConvertError::SandboxUnavailable(detail) => ToolError::SandboxUnavailable(detail),
        other => ToolError::EngineFailure(other.to_string()),
    }
}

fn extract_document_with_docling(
    input_path: &Path,
    _workspace: &Workspace,
    limits: &Limits,
    engine_program: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<Document, ToolError> {
    let (command, options) = crate::engines::resolve_pack_with_options(
        crate::engines::docling::ENGINE_ID,
        engine_program,
    )
    .map_err(|_| ToolError::from_missing_engine(crate::engines::docling::ENGINE_ID))?;

    let docling_ws = Workspace::new().map_err(|e| ToolError::OutputIo(e.to_string()))?;
    let request = Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: uuid::Uuid::new_v4().simple().to_string(),
        input: Input {
            path: input_path.to_string_lossy().into_owned(),
            format: "pdf".to_owned(),
        },
        output: Output {
            dir: docling_ws.output_dir().to_string_lossy().into_owned(),
            format: "docling+json".to_owned(),
        },
        work_dir: docling_ws.work_dir().to_string_lossy().into_owned(),
        options,
        limits: limits.clone(),
        session_id: None,
    };

    let outcome = crate::runner::run_command(&command, &request, cancel.clone(), |_| {})
        .map_err(|e| ToolError::EngineFailure(e.to_string()))?;

    let docling_artifact = outcome
        .artifacts
        .into_iter()
        .find(|a| {
            a.format == "docling+json"
                || a.path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .ok_or_else(|| ToolError::EngineFailure("Docling did not produce output".into()))?;

    let read_result = crate::convert::read_docling_to_ir(&docling_artifact.path, limits)
        .map_err(|e| ToolError::EngineFailure(format!("Failed to parse Docling output: {e}")))?;

    Ok(read_result.document)
}

/// Executes the `pdf.tables_to_csv` tool.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute(
    _spec: &ToolSpec,
    inputs: &[(PathBuf, String)],
    _params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    cancel: &CancellationToken,
    _on_event: &mut dyn FnMut(ToolEvent),
    _clock: inprocess::HostClock,
    reader: TableReader,
) -> Result<RawToolExecution, ToolError> {
    let (input_path, _) = inputs
        .first()
        .ok_or_else(|| ToolError::Usage("pdf.tables_to_csv requires one input file".into()))?;

    let (doc, read_warnings) = match reader {
        TableReader::Docling => (
            extract_document_with_docling(input_path, workspace, limits, engine_program, cancel)?,
            Vec::new(),
        ),
        TableReader::BuiltIn => {
            extract_document_in_process(input_path, limits, engine_program, cancel)?
        }
    };

    let (artifacts, mut warnings) = stage_csv_artifacts(&doc, workspace, scrubber)?;
    warnings.splice(0..0, read_warnings);
    if reader == TableReader::BuiltIn {
        warnings.push(ConvertWarning {
            code: tables_csv::DOCLING_FALLBACK_CODE.to_owned(),
            message: tables_csv::RULED_ONLY_MESSAGE.to_owned(),
        });
    }
    Ok(RawToolExecution {
        artifacts,
        warnings,
        metrics: BTreeMap::new(),
        inline_json: None,
        companion_report: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::ir::{Alignment, Block, ColumnSpec, Inline, TableCell};
    use std::fs;

    fn make_cell(text: &str) -> TableCell {
        TableCell {
            rowspan: 1,
            colspan: 1,
            header: None,
            blocks: vec![Block::Paragraph {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                content: vec![Inline::Text {
                    text: text.to_owned(),
                }],
            }],
        }
    }

    #[test]
    fn tables_to_csv_fails_when_no_tables_found() {
        let ws = Workspace::new().unwrap();
        let doc = Document {
            body: vec![Block::Paragraph {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                content: vec![Inline::Text {
                    text: "Just text".into(),
                }],
            }],
            ..Default::default()
        }
        .finalize_import();
        let err = tables_to_csv_artifacts(&doc, &ws).expect_err("should fail with no tables");
        match err {
            ToolError::EngineFailure(msg) => {
                assert!(msg.contains("no tables found in document"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn tables_to_csv_exports_tables_with_formula_guarding() {
        let ws = Workspace::new().unwrap();
        let table_block = Block::Table {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            caption: None,
            columns: vec![
                ColumnSpec {
                    align: Alignment::Default,
                },
                ColumnSpec {
                    align: Alignment::Default,
                },
            ],
            head: vec![vec![make_cell("Col A"), make_cell("=2+2")]],
            body: vec![vec![make_cell("data"), make_cell("@bad_macro")]],
            footnotes: Vec::new(),
        };
        let doc = Document {
            body: vec![table_block],
            ..Default::default()
        }
        .finalize_import();
        let (artifacts, warnings) =
            tables_to_csv_artifacts(&doc, &ws).expect("should succeed with tables");
        assert_eq!(artifacts.len(), 1);
        assert!(warnings.is_empty());
        let content = fs::read_to_string(&artifacts[0].0).unwrap();
        assert!(content.contains("'=2+2"));
        assert!(content.contains("'@bad_macro"));
    }

    #[test]
    fn tables_to_csv_finds_nested_tables_inside_quotes() {
        let ws = Workspace::new().unwrap();
        let table_block = Block::Table {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            caption: None,
            columns: vec![ColumnSpec {
                align: Alignment::Default,
            }],
            head: vec![],
            body: vec![vec![make_cell("Nested Table Cell")]],
            footnotes: Vec::new(),
        };
        let doc = Document {
            body: vec![Block::Quote {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                blocks: vec![table_block],
            }],
            ..Default::default()
        }
        .finalize_import();
        let (artifacts, _warnings) =
            tables_to_csv_artifacts(&doc, &ws).expect("should find nested table in quote");
        assert_eq!(artifacts.len(), 1);
        let content = fs::read_to_string(&artifacts[0].0).unwrap();
        assert!(content.contains("Nested Table Cell"));
    }

    #[test]
    fn tables_to_csv_caps_at_255_and_emits_warning() {
        let ws = Workspace::new().unwrap();
        let tables: Vec<Block> = (0..260)
            .map(|i| Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![],
                body: vec![vec![make_cell(&format!("cell {i}"))]],
                footnotes: Vec::new(),
            })
            .collect();
        let doc = Document {
            body: tables,
            ..Default::default()
        }
        .finalize_import();
        let (artifacts, warnings) =
            tables_to_csv_artifacts(&doc, &ws).expect("should succeed and cap at 255");
        assert_eq!(artifacts.len(), 255);
        assert!(warnings.iter().any(|w| {
            w.code == "unsupported_node"
                && w.message
                    .contains("document has 260 tables; capped at maximum 255 tables")
        }));
    }

    #[test]
    fn tables_to_csv_returns_limit_exceeded_when_bytes_cap_exceeded() {
        let ws = Workspace::new().unwrap();
        let chunk = "A".repeat(30_000);
        let rows: Vec<Vec<TableCell>> = (0..1800).map(|_| vec![make_cell(&chunk)]).collect();
        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![],
                body: rows,
                footnotes: Vec::new(),
            }],
            ..Default::default()
        }
        .finalize_import();
        let res = tables_to_csv_artifacts(&doc, &ws);
        assert!(matches!(res, Err(ToolError::LimitExceeded)));
    }

    #[cfg(unix)]
    #[test]
    fn tables_to_csv_does_not_follow_planted_symlink() {
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("victim.csv");
        fs::write(&victim, b"untouched-tables-victim").unwrap();

        let ws = Workspace::new().unwrap();
        let planted = ws.output_dir().join("table-001.csv");
        std::os::unix::fs::symlink(&victim, &planted).unwrap();

        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![vec![make_cell("Header")]],
                body: vec![vec![make_cell("value")]],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        }
        .finalize_import();
        let (artifacts, _) =
            tables_to_csv_artifacts(&doc, &ws).expect("staging replaces the planted link");

        assert_eq!(fs::read(&victim).unwrap(), b"untouched-tables-victim");
        assert_eq!(artifacts[0].0, planted);
        let meta = fs::symlink_metadata(&planted).unwrap();
        assert!(!meta.file_type().is_symlink() && meta.file_type().is_file());
        assert!(fs::read_to_string(&planted).unwrap().contains("value"));
    }
}
