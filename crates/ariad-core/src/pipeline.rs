//! In-memory conversion pipeline for the in-process `ariad-core` engine.
//!
//! Readers and writers are selected by [`Format`]; a new in-process reader or writer
//! plugs in as one arm of [`read`] or [`write`]. Nothing here touches the file system,
//! so the browser binding and any other in-process host share the same behavior.

use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    format::Format,
    ir::{AssetRef, Block, Document, IR_VERSION, Inline},
    json_depth::{json_depth_budget_for_nesting, prescan_json_depth},
    limits::{Limits, LimitsError},
    planner::PlanStep,
    reader::{html as reader_html, markdown as reader_markdown},
    warning::{SourcePos, Warning},
    writer::{html as writer_html, markdown as writer_markdown},
};

/// Engine identifier of the in-process readers and writers in this crate.
pub const ENGINE: &str = "ariad-core";

/// A non-fatal finding reported by a reader, a writer or the pipeline itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Notice {
    /// Stable snake_case identifier, for example `raw_dropped`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// One-based source position, when the reader knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_pos: Option<SourcePos>,
}

impl Notice {
    /// Creates a notice without a source position.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            source_pos: None,
        }
    }
}

impl From<&Warning> for Notice {
    fn from(warning: &Warning) -> Self {
        let code = serde_json::to_value(warning.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "conversion_warning".to_owned());
        Self {
            code,
            message: warning.message.clone(),
            source_pos: warning.source_pos,
        }
    }
}

/// Typed failure of an in-memory conversion step.
///
/// Serializes as an object whose `code` field names the variant.
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PipelineError {
    /// The format id or the input content is not a known format.
    #[error("{}", unknown_format_message(.format.as_deref()))]
    UnknownFormat {
        /// The rejected format id, when one was given.
        format: Option<String>,
    },
    /// A step needs an engine that this pipeline does not run.
    #[error("the {engine} engine is not available here")]
    EngineUnavailable {
        /// Engine identifier, for example `pandoc`.
        engine: String,
    },
    /// The in-process engine has no reader for this format.
    #[error("{} cannot be read here", .format.id())]
    ReaderUnavailable {
        /// The unreadable format.
        format: Format,
    },
    /// The in-process engine has no writer for this format.
    #[error("{} cannot be written here", .format.id())]
    WriterUnavailable {
        /// The unwritable format.
        format: Format,
    },
    /// The input is not valid for its format, for example Markdown that is not UTF-8.
    #[error("invalid input: {reason}")]
    InvalidInput {
        /// What is wrong with the input.
        reason: String,
    },
    /// The input is not a readable AriadShift IR document.
    #[error("invalid IR document: {reason}")]
    InvalidIr {
        /// What is wrong with the document.
        reason: String,
    },
    /// The IR document declares a version this build does not read.
    #[error("unsupported IR version {}; expected {expected}", .found.as_deref().unwrap_or("(missing)"))]
    UnsupportedIrVersion {
        /// The declared version, if any.
        found: Option<String>,
        /// The version this build reads.
        expected: String,
    },
    /// The input exceeds a configured limit.
    #[error("the input exceeds the {limit} limit")]
    LimitExceeded {
        /// Name of the exceeded limit, for example `max_blocks`.
        limit: String,
    },
    /// The supplied limits are not usable.
    #[error("invalid limits: {reason}")]
    InvalidLimits {
        /// Why the limits were rejected.
        reason: String,
    },
}

fn unknown_format_message(format: Option<&str>) -> String {
    match format {
        Some(format) => format!("unknown format '{format}'"),
        None => "the input format could not be detected".to_owned(),
    }
}

impl From<LimitsError> for PipelineError {
    fn from(error: LimitsError) -> Self {
        Self::InvalidLimits {
            reason: error.to_string(),
        }
    }
}

fn limit_exceeded(limit: &str) -> PipelineError {
    PipelineError::LimitExceeded {
        limit: limit.to_owned(),
    }
}

/// A document read into the IR, with the reader's notices.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub document: Document,
    pub warnings: Vec<Notice>,
}

/// Serialized output of a writer, with its notices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Written {
    pub bytes: Vec<u8>,
    pub warnings: Vec<Notice>,
}

/// Options that writers may use.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WriteOptions<'a> {
    /// Title used when the document has none, typically the input file stem.
    pub title_fallback: Option<&'a str>,
}

/// Whether [`read`] supports `format`.
#[must_use]
pub const fn can_read(format: Format) -> bool {
    matches!(
        format,
        Format::Markdown | Format::Html | Format::AriadIrJson
    )
}

/// Whether [`write`] supports `format`.
#[must_use]
pub const fn can_write(format: Format) -> bool {
    matches!(
        format,
        Format::Markdown | Format::Html | Format::AriadIrJson
    )
}

/// Reads `bytes` of `format` into the IR.
pub fn read(format: Format, bytes: &[u8], limits: &Limits) -> Result<Read, PipelineError> {
    limits.validate()?;
    if let Some(limit) = limits.max_input_bytes
        && bytes.len() as u64 > limit
    {
        return Err(limit_exceeded("max_input_bytes"));
    }
    let output = match format {
        Format::Markdown => {
            let text = std::str::from_utf8(bytes).map_err(|error| PipelineError::InvalidInput {
                reason: format!(
                    "Markdown must be UTF-8; invalid byte at offset {}",
                    error.valid_up_to()
                ),
            })?;
            reader_markdown::read(text, limits).map_err(|error| match error {
                reader_markdown::ReadError::InvalidLimits(error) => error.into(),
                reader_markdown::ReadError::InputTooLarge { .. } => {
                    limit_exceeded("max_input_bytes")
                }
                reader_markdown::ReadError::NestingTooDeep { .. } => {
                    limit_exceeded("max_nesting_depth")
                }
                reader_markdown::ReadError::TooManyBlocks { .. } => limit_exceeded("max_blocks"),
            })?
        }
        Format::Html => reader_html::read(bytes, limits).map_err(|error| match error {
            reader_html::ReadError::InvalidLimits(error) => error.into(),
            reader_html::ReadError::InputTooLarge { .. } => limit_exceeded("max_input_bytes"),
            reader_html::ReadError::NestingTooDeep { .. } => limit_exceeded("max_nesting_depth"),
            reader_html::ReadError::TooManyBlocks { .. } => limit_exceeded("max_blocks"),
            reader_html::ReadError::TooManyNodes { .. } => limit_exceeded("max_dom_nodes"),
        })?,
        Format::AriadIrJson => {
            return Ok(Read {
                document: read_ir_json(bytes, limits)?,
                warnings: Vec::new(),
            });
        }
        other => return Err(PipelineError::ReaderUnavailable { format: other }),
    };

    let mut warnings: Vec<Notice> = output.warnings.iter().map(Notice::from).collect();
    for _ in 0..count_linked_images(&output.document) {
        warnings.push(Notice::new(
            "image_not_embedded",
            "An image was kept as a link because only the selected file is available.",
        ));
    }
    Ok(Read {
        document: output.document,
        warnings,
    })
}

/// Writes `document` as `format`.
pub fn write(
    format: Format,
    document: &Document,
    options: WriteOptions<'_>,
) -> Result<Written, PipelineError> {
    let output = match format {
        Format::Markdown => writer_markdown::write(document),
        Format::Html => writer_html::write_with_title_fallback(document, options.title_fallback),
        Format::AriadIrJson => {
            let bytes =
                serde_json::to_vec_pretty(document).map_err(|error| PipelineError::InvalidIr {
                    reason: error.to_string(),
                })?;
            return Ok(Written {
                bytes,
                warnings: Vec::new(),
            });
        }
        other => return Err(PipelineError::WriterUnavailable { format: other }),
    };
    Ok(Written {
        bytes: output.content.into_bytes(),
        warnings: output.warnings.iter().map(Notice::from).collect(),
    })
}

/// Reads an IR JSON document with the same bounds as the native host: byte cap,
/// nesting pre-scan before deserializing, version check and block count.
pub fn read_ir_json(bytes: &[u8], limits: &Limits) -> Result<Document, PipelineError> {
    limits.validate()?;
    if let Some(limit) = limits.max_input_bytes
        && bytes.len() as u64 > limit
    {
        return Err(limit_exceeded("max_input_bytes"));
    }
    if bytes.len() as u64 > limits.max_ir_json_bytes {
        return Err(limit_exceeded("max_ir_json_bytes"));
    }
    let budget = json_depth_budget_for_nesting(limits.max_nesting_depth);
    prescan_json_depth(bytes, budget).map_err(|_| limit_exceeded("max_nesting_depth"))?;

    #[derive(Deserialize)]
    struct VersionProbe {
        version: Option<String>,
    }
    let probe: VersionProbe = deserialize_bounded(bytes).map_err(|error| {
        if error.is_data() {
            PipelineError::InvalidIr {
                reason: "the JSON is not an AriadShift IR document".to_owned(),
            }
        } else {
            invalid_ir(&error)
        }
    })?;
    if probe.version.as_deref() != Some(IR_VERSION) {
        return Err(PipelineError::UnsupportedIrVersion {
            found: probe.version,
            expected: IR_VERSION.to_owned(),
        });
    }

    let document: Document = deserialize_bounded(bytes).map_err(|error| invalid_ir(&error))?;
    if document.block_count() > limits.max_blocks as usize {
        return Err(limit_exceeded("max_blocks"));
    }
    Ok(document)
}

/// Deserializes JSON whose nesting a caller has already bounded with a pre-scan.
fn deserialize_bounded<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> serde_json::Result<T> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let value = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

fn invalid_ir(error: &serde_json::Error) -> PipelineError {
    PipelineError::InvalidIr {
        reason: error.to_string(),
    }
}

/// Final output of [`execute`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Converted {
    pub bytes: Vec<u8>,
    pub warnings: Vec<Notice>,
}

enum Stage<'a> {
    Bytes(Format, Cow<'a, [u8]>),
    Document(Box<Document>),
}

/// Runs a planned route whose every step uses the in-process engine.
///
/// The first step reads `bytes`; a route that ends at the IR returns pretty-printed IR JSON.
pub fn execute(
    bytes: &[u8],
    steps: &[PlanStep],
    limits: &Limits,
    options: WriteOptions<'_>,
) -> Result<Converted, PipelineError> {
    let Some(first) = steps.first() else {
        return Err(PipelineError::InvalidInput {
            reason: "the route has no steps".to_owned(),
        });
    };
    if let Some(step) = steps.iter().find(|step| step.engine != ENGINE) {
        return Err(PipelineError::EngineUnavailable {
            engine: step.engine.clone(),
        });
    }

    let mut warnings = Vec::new();
    let mut stage = Stage::Bytes(first.from, Cow::Borrowed(bytes));
    for step in steps {
        stage = match (stage, step.from, step.to) {
            (Stage::Bytes(format, input), _, Format::AriadIrJson) => {
                let read = read(format, &input, limits)?;
                warnings.extend(read.warnings);
                Stage::Document(Box::new(read.document))
            }
            (Stage::Bytes(Format::AriadIrJson, input), Format::AriadIrJson, to) => {
                let document = read_ir_json(&input, limits)?;
                let written = write(to, &document, options)?;
                warnings.extend(written.warnings);
                Stage::Bytes(to, Cow::Owned(written.bytes))
            }
            (Stage::Document(document), Format::AriadIrJson, to) => {
                let written = write(to, &document, options)?;
                warnings.extend(written.warnings);
                Stage::Bytes(to, Cow::Owned(written.bytes))
            }
            (_, from, to) => {
                return Err(PipelineError::InvalidInput {
                    reason: format!(
                        "the in-process engine has no step from {} to {}",
                        from.id(),
                        to.id()
                    ),
                });
            }
        };
    }

    let bytes = match stage {
        Stage::Bytes(_, bytes) => bytes.into_owned(),
        Stage::Document(document) => write(Format::AriadIrJson, &document, options)?.bytes,
    };
    Ok(Converted { bytes, warnings })
}

/// Counts image references that remain URLs, iteratively to stay stack-safe.
fn count_linked_images(document: &Document) -> usize {
    let mut count = 0;
    let mut blocks: Vec<&Block> = document.body.iter().chain(&document.furniture).collect();
    let mut inlines: Vec<&Inline> = Vec::new();
    while let Some(block) = blocks.pop() {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                inlines.extend(content);
            }
            Block::Quote { blocks: nested } | Block::Footnote { blocks: nested, .. } => {
                blocks.extend(nested);
            }
            Block::List { items, .. } => {
                for item in items {
                    blocks.extend(&item.blocks);
                }
            }
            Block::Table {
                caption,
                head,
                body,
                footnotes,
                ..
            } => {
                if let Some(caption) = caption {
                    inlines.extend(caption);
                }
                inlines.extend(footnotes);
                for row in head.iter().chain(body) {
                    for cell in row {
                        blocks.extend(&cell.blocks);
                    }
                }
            }
            Block::Figure { asset, caption } => {
                if matches!(asset, AssetRef::Url { .. }) {
                    count += 1;
                }
                inlines.extend(caption);
            }
            Block::Code { .. } | Block::Math { .. } | Block::PageBreak {} | Block::Raw { .. } => {}
        }
        while let Some(inline) = inlines.pop() {
            match inline {
                Inline::Emph { content }
                | Inline::Strong { content }
                | Inline::Strikeout { content }
                | Inline::Superscript { content }
                | Inline::Subscript { content }
                | Inline::Link { content, .. } => inlines.extend(content),
                Inline::Image { target, .. } => {
                    if matches!(target, AssetRef::Url { .. }) {
                        count += 1;
                    }
                }
                Inline::Text { .. }
                | Inline::Code { .. }
                | Inline::SoftBreak {}
                | Inline::LineBreak {}
                | Inline::Math { .. }
                | Inline::FootnoteRef { .. }
                | Inline::Raw { .. } => {}
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(from: Format, to: Format) -> PlanStep {
        PlanStep {
            from,
            to,
            engine: ENGINE.to_owned(),
        }
    }

    fn markdown_to_html() -> Vec<PlanStep> {
        vec![
            step(Format::Markdown, Format::AriadIrJson),
            step(Format::AriadIrJson, Format::Html),
        ]
    }

    #[test]
    fn converts_markdown_to_html_with_title_fallback() {
        let converted = execute(
            b"Hello *world*\n",
            &markdown_to_html(),
            &Limits::local(),
            WriteOptions {
                title_fallback: Some("notes"),
            },
        )
        .unwrap();
        let html = String::from_utf8(converted.bytes).unwrap();
        assert!(html.contains("<title>notes</title>"), "{html}");
        assert!(html.contains("<em>world</em>"), "{html}");
        assert!(converted.warnings.is_empty());
    }

    #[test]
    fn round_trips_markdown_through_ir_json() {
        let source = b"# Title\n\n- one\n- two\n";
        let ir = execute(
            source,
            &[step(Format::Markdown, Format::AriadIrJson)],
            &Limits::local(),
            WriteOptions::default(),
        )
        .unwrap();
        let ir_text = String::from_utf8(ir.bytes.clone()).unwrap();
        assert!(ir_text.contains("\"version\": \"ariad-ir/0\""), "{ir_text}");

        let markdown = execute(
            &ir.bytes,
            &[step(Format::AriadIrJson, Format::Markdown)],
            &Limits::local(),
            WriteOptions::default(),
        )
        .unwrap();
        let direct = execute(
            source,
            &[
                step(Format::Markdown, Format::AriadIrJson),
                step(Format::AriadIrJson, Format::Markdown),
            ],
            &Limits::local(),
            WriteOptions::default(),
        )
        .unwrap();
        assert_eq!(markdown.bytes, direct.bytes);
    }

    #[test]
    fn refuses_steps_of_other_engines() {
        let steps = [PlanStep {
            from: Format::AriadIrJson,
            to: Format::Docx,
            engine: "pandoc".to_owned(),
        }];
        let error = execute(b"{}", &steps, &Limits::local(), WriteOptions::default()).unwrap_err();
        assert_eq!(
            error,
            PipelineError::EngineUnavailable {
                engine: "pandoc".to_owned()
            }
        );
    }

    #[test]
    fn rejects_markdown_that_is_not_utf8() {
        let error = read(Format::Markdown, b"ok \xff", &Limits::local()).unwrap_err();
        assert!(matches!(error, PipelineError::InvalidInput { .. }));
    }

    #[test]
    fn enforces_input_and_block_limits() {
        let limits = Limits {
            max_input_bytes: Some(4),
            ..Limits::local()
        };
        let error = read(Format::Html, b"<p>long</p>", &limits).unwrap_err();
        assert_eq!(error, limit_exceeded("max_input_bytes"));

        let limits = Limits {
            max_blocks: 1,
            ..Limits::local()
        };
        let error = read(Format::Markdown, b"one\n\ntwo\n\nthree\n", &limits).unwrap_err();
        assert_eq!(error, limit_exceeded("max_blocks"));
    }

    #[test]
    fn checks_ir_version_and_shape() {
        let error =
            read_ir_json(br#"{"version":"ariad-ir/9","body":[]}"#, &Limits::local()).unwrap_err();
        assert_eq!(
            error,
            PipelineError::UnsupportedIrVersion {
                found: Some("ariad-ir/9".to_owned()),
                expected: IR_VERSION.to_owned(),
            }
        );
        let error = read_ir_json(b"[1, 2]", &Limits::local()).unwrap_err();
        assert!(matches!(error, PipelineError::InvalidIr { .. }));
        let error = read_ir_json(br#"{"version": "ariad-ir/0", "#, &Limits::local()).unwrap_err();
        assert!(matches!(error, PipelineError::InvalidIr { .. }));
    }

    #[test]
    fn applies_the_input_cap_to_ir_json_routes() {
        let limits = Limits {
            max_input_bytes: Some(10),
            ..Limits::local()
        };
        let ir = serde_json::to_vec(&Document::default()).unwrap();
        let error = execute(
            &ir,
            &[step(Format::AriadIrJson, Format::Html)],
            &limits,
            WriteOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error, limit_exceeded("max_input_bytes"));
    }

    #[test]
    fn rejects_ir_json_nested_beyond_the_budget() {
        let limits = Limits {
            max_nesting_depth: 2,
            ..Limits::local()
        };
        let depth = json_depth_budget_for_nesting(2) + 1;
        let json = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let error = read_ir_json(json.as_bytes(), &limits).unwrap_err();
        assert_eq!(error, limit_exceeded("max_nesting_depth"));
    }

    #[test]
    fn reads_deep_ir_within_the_local_budget() {
        // Deep documents need more stack than the default test thread provides.
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let mut block = Block::Paragraph { content: vec![] };
                for _ in 0..60 {
                    block = Block::Quote {
                        blocks: vec![block],
                    };
                }
                let document = Document {
                    body: vec![block],
                    ..Document::default()
                };
                let bytes = serde_json::to_vec(&document).unwrap();
                let read = read_ir_json(&bytes, &Limits::local()).unwrap();
                assert_eq!(read, document);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn reports_linked_images() {
        let read = read(
            Format::Markdown,
            b"![logo](logo.png) and ![remote](https://example.test/a.png)\n",
            &Limits::local(),
        )
        .unwrap();
        let codes: Vec<&str> = read.warnings.iter().map(|w| w.code.as_str()).collect();
        assert_eq!(codes, ["image_not_embedded", "image_not_embedded"]);
    }

    #[test]
    fn errors_serialize_with_a_code_field() {
        let value = serde_json::to_value(limit_exceeded("max_blocks")).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"code": "limit_exceeded", "limit": "max_blocks"})
        );
        let value = serde_json::to_value(PipelineError::ReaderUnavailable {
            format: Format::Docx,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"code": "reader_unavailable", "format": "docx"})
        );
    }

    #[test]
    fn readers_and_writers_report_unsupported_formats() {
        assert!(!can_read(Format::Docx));
        assert!(!can_write(Format::Pdf));
        let error = read(Format::Docx, b"PK", &Limits::local()).unwrap_err();
        assert_eq!(
            error,
            PipelineError::ReaderUnavailable {
                format: Format::Docx
            }
        );
        let error = write(Format::Epub, &Document::default(), WriteOptions::default()).unwrap_err();
        assert_eq!(
            error,
            PipelineError::WriterUnavailable {
                format: Format::Epub
            }
        );
    }
}
