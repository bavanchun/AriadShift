//! Central dispatcher for in-process `ariad-pdf` tools shared by `ariad-host` and `ariad-wasm`.
//!
//! Dispatches `pdf.info` and all 25 structural PDF tools through [`run_tool`], enforcing:
//! - Parameter deserialization into the strongly typed `ariad_core::tools::params::*Params` struct.
//! - Bounded PDF loading via [`crate::document::open_with_ctx`] (refusing encrypted inputs with
//!   [`PdfError::Encrypted`]).
//! - Central injection of [`REPAIRED_XREF_WARNING`] into [`ToolOutput::warnings`] whenever any
//!   opened input required cross-reference table reconstruction (`doc.repaired_xref() == true`).
//! - Deterministic PDF serialization of [`OutputDoc`] artifacts via [`crate::writer::save_with_options`].

pub mod alternate_mix;
pub mod attachments;
pub mod booklet;
pub mod crop;
pub mod decompress;
pub mod delete_pages;
pub mod divide_pages;
pub mod extract_pages;
pub mod insert_blank;
pub mod layers;
pub mod merge;
pub mod metadata;
pub mod nup;
pub mod outline;
pub mod overlay;
pub mod page_labels;
pub(crate) mod page_set;
pub mod portfolio;
pub mod posterize;
pub mod reorder;
pub mod rotate;
pub mod rotate_content;
pub mod scale;
pub mod single_page;
pub mod split;
pub mod viewer_prefs;

use ariad_core::{
    limits::Limits,
    tools::params::{
        AlternateMixParams, AttachmentsParams, BookletParams, CropParams, DecompressParams,
        DeletePagesParams, DividePagesParams, ExtractPagesParams, InfoParams, InsertBlankParams,
        LayersParams, MergeParams, MetadataParams, NupParams, OutlineParams, OverlayParams,
        PageLabelsParams, PortfolioParams, PosterizeParams, ReorderParams, RotateContentParams,
        RotateParams, ScaleParams, SinglePageParams, SplitParams, ViewerPrefsParams,
    },
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    document::{self, PdfDoc, ToolCtx},
    error::PdfError,
    info,
    naming::{self, default_pdf_output_name, part_output_name, sanitize_stem, single_output_name},
    writer::{self, SaveOptions},
};

/// Normative diagnostic warning emitted whenever an input PDF's cross-reference table was
/// malformed and reconstructed from object offsets (`docs/tools.md`).
pub const REPAIRED_XREF_WARNING: &str =
    "cross-reference table was malformed and reconstructed from object offsets";

/// Returns a human-readable message for an encrypted PDF input pointing to `pdf.decrypt`.
#[must_use]
pub const fn encrypted_error_message(needs_password: bool) -> &'static str {
    if needs_password {
        "the PDF is encrypted (password required); decrypt it with `pdf.decrypt` first"
    } else {
        "the PDF is encrypted (owner-password protected); decrypt it with `pdf.decrypt` first"
    }
}

/// Source payload for an [`InputDoc`]: either raw PDF bytes or an already-opened [`PdfDoc`].
#[derive(Clone, Debug)]
pub enum InputSource {
    /// Raw input file bytes to be opened with [`document::open_with_ctx`].
    Bytes(Vec<u8>),
    /// Pre-opened [`PdfDoc`] handle.
    Doc(Box<PdfDoc>),
}

/// A single input document passed to [`run_tool`].
#[derive(Clone, Debug)]
pub struct InputDoc {
    /// Input filename or stem (used for default output naming and per-file merge bookmarks).
    pub stem: String,
    /// Input bytes or pre-opened [`PdfDoc`].
    pub source: InputSource,
    /// Resource limits enforced when opening raw bytes.
    pub limits: Limits,
}

impl InputDoc {
    /// Creates an [`InputDoc`] from a filename or stem and raw PDF bytes under [`Limits::local()`].
    #[must_use]
    pub fn new(stem: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            stem: stem.into(),
            source: InputSource::Bytes(bytes),
            limits: Limits::local(),
        }
    }

    /// Creates an [`InputDoc`] from a filename or stem, raw PDF bytes, and explicit [`Limits`].
    #[must_use]
    pub fn with_limits(stem: impl Into<String>, bytes: Vec<u8>, limits: Limits) -> Self {
        Self {
            stem: stem.into(),
            source: InputSource::Bytes(bytes),
            limits,
        }
    }

    /// Creates an [`InputDoc`] from an already-opened [`PdfDoc`].
    #[must_use]
    pub fn from_doc(stem: impl Into<String>, doc: PdfDoc) -> Self {
        Self {
            stem: stem.into(),
            source: InputSource::Doc(Box::new(doc)),
            limits: Limits::local(),
        }
    }

    /// Returns the sanitized file stem of this input.
    #[must_use]
    pub fn clean_stem(&self) -> String {
        sanitize_stem(&self.stem)
    }

    /// Opens or clones the underlying [`PdfDoc`] for a mutating structural tool, refusing
    /// encrypted documents with [`PdfError::Encrypted`].
    pub fn open_for_mutation(&self, ctx: &ToolCtx) -> Result<PdfDoc, PdfError> {
        let mut doc = match &self.source {
            InputSource::Bytes(bytes) => document::open_with_ctx(bytes, &self.limits, ctx)?,
            InputSource::Doc(existing) => {
                ctx.check()?;
                if existing.is_encrypted() {
                    return Err(PdfError::Encrypted {
                        needs_password: false,
                    });
                }
                (**existing).clone()
            }
        };
        naming::set_doc_stem(&mut doc, &self.stem);
        Ok(doc)
    }

    /// Opens or borrows the underlying [`PdfDoc`] in read-only inspection mode (`pdf.info`).
    pub fn open_for_info(&self, ctx: &ToolCtx) -> Result<PdfDoc, PdfError> {
        match &self.source {
            InputSource::Bytes(bytes) => document::open_for_info_with_ctx(bytes, &self.limits, ctx),
            InputSource::Doc(existing) => {
                ctx.check()?;
                Ok((**existing).clone())
            }
        }
    }
}

/// Payload carried by an [`OutputDoc`] returned from a per-tool `run` function.
#[derive(Clone, Debug)]
pub enum OutputPayload {
    /// A [`PdfDoc`] to be saved deterministically via [`writer::save_with_options`].
    Pdf(Box<PdfDoc>),
    /// Pre-serialized artifact bytes (for example JSON from `pdf.outline` export).
    Bytes(Vec<u8>),
}

/// An output document or artifact returned by a per-tool `run` implementation prior to final
/// serialization and warning aggregation.
#[derive(Clone, Debug)]
pub struct OutputDoc {
    /// Output artifact filename (when empty, [`finalize_tool_output`] assigns the normative
    /// `<stem>.<suffix>.pdf` or `<stem>.part-NNN.pdf` name).
    pub name: String,
    /// Output format identifier (`"pdf"`, `"json"`, etc.).
    pub format: String,
    /// Output payload (`PdfDoc` or raw bytes).
    pub payload: OutputPayload,
    /// Tool-specific warnings for this output.
    pub warnings: Vec<String>,
    /// Optional structured JSON value when `format == "json"`.
    pub json: Option<Value>,
    /// Deterministic writer options when `payload` is [`OutputPayload::Pdf`].
    pub save_options: SaveOptions,
}

impl OutputDoc {
    /// Constructs an [`OutputDoc`] from a [`PdfDoc`] with an explicit filename.
    #[must_use]
    pub fn from_pdf(name: impl Into<String>, doc: PdfDoc) -> Self {
        Self {
            name: name.into(),
            format: "pdf".to_owned(),
            payload: OutputPayload::Pdf(Box::new(doc)),
            warnings: Vec::new(),
            json: None,
            save_options: SaveOptions::default(),
        }
    }

    /// Constructs an [`OutputDoc`] from a [`PdfDoc`] using the tool's default naming rule.
    #[must_use]
    pub fn unnamed_pdf(doc: PdfDoc) -> Self {
        Self::from_pdf(String::new(), doc)
    }

    /// Constructs a JSON [`OutputDoc`] from a [`serde_json::Value`].
    pub fn from_json(name: impl Into<String>, value: Value) -> Result<Self, PdfError> {
        let bytes = serde_json::to_vec(&value).map_err(|e| {
            PdfError::InvalidParams(format!("failed to serialize JSON output: {e}"))
        })?;
        Ok(Self {
            name: name.into(),
            format: "json".to_owned(),
            payload: OutputPayload::Bytes(bytes),
            warnings: Vec::new(),
            json: Some(value),
            save_options: SaveOptions::default(),
        })
    }

    /// Constructs an [`OutputDoc`] from raw bytes and a format identifier.
    #[must_use]
    pub fn from_bytes(name: impl Into<String>, format: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            format: format.into(),
            payload: OutputPayload::Bytes(bytes),
            warnings: Vec::new(),
            json: None,
            save_options: SaveOptions::default(),
        }
    }

    /// Appends a non-fatal warning message to this output.
    #[must_use]
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Sets custom [`SaveOptions`] for deterministic PDF writing.
    #[must_use]
    pub const fn with_save_options(mut self, save_options: SaveOptions) -> Self {
        self.save_options = save_options;
        self
    }
}

/// A serialized output artifact produced by [`run_tool`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputArtifact {
    /// Default output filename (for example `"report.rotated.pdf"` or `"report.part-001.pdf"`).
    pub name: String,
    /// Output format identifier (`"pdf"`, `"json"`, etc.).
    pub format: String,
    /// Serialized artifact bytes.
    pub bytes: Vec<u8>,
}

/// Complete output returned by [`run_tool`].
#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutput {
    /// Produced output artifacts in deterministic order.
    pub artifacts: Vec<OutputArtifact>,
    /// Non-fatal diagnostic warnings (including [`REPAIRED_XREF_WARNING`] when any input had a
    /// repaired cross-reference table).
    pub warnings: Vec<String>,
    /// Optional structured JSON payload for JSON-emitting tools (`pdf.info`, `pdf.outline` export, etc.).
    pub json: Option<Value>,
}

/// Executes an in-process `ariad-pdf` tool by `tool_id` on `inputs` with `params` and cooperative
/// `ctx`.
pub fn run_tool(
    tool_id: &str,
    inputs: &[InputDoc],
    params: &Value,
    ctx: &ToolCtx,
) -> Result<ToolOutput, PdfError> {
    ctx.check()?;

    if tool_id == "pdf.info" {
        return run_pdf_info(inputs, params, ctx);
    }

    match tool_id {
        "pdf.merge" => {
            dispatch_structural::<MergeParams, _>(tool_id, inputs, params, ctx, merge::run)
        }
        "pdf.split" => {
            dispatch_structural::<SplitParams, _>(tool_id, inputs, params, ctx, split::run)
        }
        "pdf.extract_pages" => dispatch_structural::<ExtractPagesParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            extract_pages::run,
        ),
        "pdf.delete_pages" => dispatch_structural::<DeletePagesParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            delete_pages::run,
        ),
        "pdf.reorder" => {
            dispatch_structural::<ReorderParams, _>(tool_id, inputs, params, ctx, reorder::run)
        }
        "pdf.alternate_mix" => dispatch_structural::<AlternateMixParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            alternate_mix::run,
        ),
        "pdf.insert_blank" => dispatch_structural::<InsertBlankParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            insert_blank::run,
        ),
        "pdf.rotate" => {
            dispatch_structural::<RotateParams, _>(tool_id, inputs, params, ctx, rotate::run)
        }
        "pdf.crop" => dispatch_structural::<CropParams, _>(tool_id, inputs, params, ctx, crop::run),
        "pdf.scale" => {
            dispatch_structural::<ScaleParams, _>(tool_id, inputs, params, ctx, scale::run)
        }
        "pdf.single_page" => dispatch_structural::<SinglePageParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            single_page::run,
        ),
        "pdf.rotate_content" => dispatch_structural::<RotateContentParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            rotate_content::run,
        ),
        "pdf.decompress" => dispatch_structural::<DecompressParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            decompress::run,
        ),
        "pdf.nup" => dispatch_structural::<NupParams, _>(tool_id, inputs, params, ctx, nup::run),
        "pdf.booklet" => {
            dispatch_structural::<BookletParams, _>(tool_id, inputs, params, ctx, booklet::run)
        }
        "pdf.overlay" => {
            dispatch_structural::<OverlayParams, _>(tool_id, inputs, params, ctx, overlay::run)
        }
        "pdf.divide_pages" => dispatch_structural::<DividePagesParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            divide_pages::run,
        ),
        "pdf.posterize" => {
            dispatch_structural::<PosterizeParams, _>(tool_id, inputs, params, ctx, posterize::run)
        }
        "pdf.metadata" => {
            dispatch_structural::<MetadataParams, _>(tool_id, inputs, params, ctx, metadata::run)
        }
        "pdf.outline" => {
            dispatch_structural::<OutlineParams, _>(tool_id, inputs, params, ctx, outline::run)
        }
        "pdf.attachments" => dispatch_structural::<AttachmentsParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            attachments::run,
        ),
        "pdf.page_labels" => dispatch_structural::<PageLabelsParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            page_labels::run,
        ),
        "pdf.viewer_prefs" => dispatch_structural::<ViewerPrefsParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            viewer_prefs::run,
        ),
        "pdf.portfolio" => {
            dispatch_structural::<PortfolioParams, _>(tool_id, inputs, params, ctx, portfolio::run)
        }
        "pdf.layers" => {
            dispatch_structural::<LayersParams, _>(tool_id, inputs, params, ctx, layers::run)
        }
        other => Err(PdfError::InvalidParams(format!("unknown tool `{other}`"))),
    }
}

fn run_pdf_info(
    inputs: &[InputDoc],
    params: &Value,
    ctx: &ToolCtx,
) -> Result<ToolOutput, PdfError> {
    let _: InfoParams = serde_json::from_value(params.clone())
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let Some(first) = inputs.first() else {
        return Err(PdfError::InvalidParams(
            "pdf.info requires exactly 1 input document".to_owned(),
        ));
    };
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.info requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }

    let doc = first.open_for_info(ctx)?;
    let report = info::from_doc_with_ctx(&doc, ctx)?;
    let warnings = report.warnings.clone();
    let json_val =
        serde_json::to_value(&report).map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let json_bytes =
        serde_json::to_vec(&json_val).map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let name = single_output_name(&first.stem, "info", "json");

    Ok(ToolOutput {
        artifacts: vec![OutputArtifact {
            name,
            format: "json".to_owned(),
            bytes: json_bytes,
        }],
        warnings,
        json: Some(json_val),
    })
}

/// Deserializes `params` into `P`, opens all `inputs` via [`InputDoc::open_for_mutation`], invokes
/// `runner`, and finalizes output artifacts and central [`REPAIRED_XREF_WARNING`] collection.
pub fn dispatch_structural<P, F>(
    tool_id: &str,
    inputs: &[InputDoc],
    params: &Value,
    ctx: &ToolCtx,
    runner: F,
) -> Result<ToolOutput, PdfError>
where
    P: DeserializeOwned,
    F: FnOnce(&[PdfDoc], &P, &ToolCtx) -> Result<Vec<OutputDoc>, PdfError>,
{
    ctx.check()?;
    let typed_params: P = serde_json::from_value(params.clone())
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    if inputs.is_empty() {
        return Err(PdfError::InvalidParams(format!(
            "tool `{tool_id}` requires at least 1 input document"
        )));
    }

    let mut opened_docs = Vec::with_capacity(inputs.len());
    for input in inputs {
        ctx.check()?;
        opened_docs.push(input.open_for_mutation(ctx)?);
    }

    let first_stem = inputs
        .first()
        .map(InputDoc::clean_stem)
        .unwrap_or_else(|| "document".to_owned());

    let output_docs = runner(&opened_docs, &typed_params, ctx)?;
    finalize_tool_output(tool_id, &opened_docs, &first_stem, output_docs, ctx)
}

/// Serializes [`OutputDoc`]s into [`OutputArtifact`]s via [`writer::save_with_options`] and
/// centrally prepends [`REPAIRED_XREF_WARNING`] whenever any input [`PdfDoc`] had
/// `repaired_xref() == true`.
pub fn finalize_tool_output(
    tool_id: &str,
    opened_inputs: &[PdfDoc],
    first_stem: &str,
    output_docs: Vec<OutputDoc>,
    ctx: &ToolCtx,
) -> Result<ToolOutput, PdfError> {
    ctx.check()?;

    let mut warnings = Vec::new();
    if opened_inputs.iter().any(PdfDoc::repaired_xref) {
        warnings.push(REPAIRED_XREF_WARNING.to_owned());
    }

    let total_outputs = output_docs.len();
    let multi = total_outputs > 1 || tool_id == "pdf.split";
    let mut artifacts = Vec::with_capacity(total_outputs);
    let mut json_payload = None;

    for (idx, mut out_doc) in output_docs.into_iter().enumerate() {
        ctx.check()?;
        for w in out_doc.warnings {
            if !warnings.contains(&w) {
                warnings.push(w);
            }
        }
        if out_doc.json.is_some() && json_payload.is_none() {
            json_payload = out_doc.json.take();
        }

        let name = if !out_doc.name.is_empty() {
            out_doc.name
        } else if multi && out_doc.format == "pdf" {
            part_output_name(first_stem, idx + 1, total_outputs)
        } else if out_doc.format == "pdf" {
            default_pdf_output_name(first_stem, tool_id)
        } else {
            single_output_name(first_stem, naming::tool_suffix(tool_id), &out_doc.format)
        };

        let bytes = match out_doc.payload {
            OutputPayload::Pdf(mut pdf_doc) => {
                writer::save_with_options(&mut pdf_doc, ctx, &out_doc.save_options)?
            }
            OutputPayload::Bytes(raw) => raw,
        };

        artifacts.push(OutputArtifact {
            name,
            format: out_doc.format,
            bytes,
        });
    }

    Ok(ToolOutput {
        artifacts,
        warnings,
        json: json_payload,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ariad_core::reader::pdf as core_pdf;
    use serde_json::json;

    use super::*;

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools")
            .join(name);
        std::fs::read(path).expect("read fixture")
    }

    #[test]
    fn run_tool_rejects_unknown_tool_id_and_dispatches_all_25_structural_tools() {
        let sample = fixture_bytes("multipage-20.pdf");
        let rotated = fixture_bytes("rotated-pages.pdf");
        let watermark = fixture_bytes("watermark-layer.pdf");
        let input = InputDoc::new("multipage-20.pdf", sample);
        let input2 = InputDoc::new("rotated-pages.pdf", rotated);
        let watermark_input = InputDoc::new("watermark-layer.pdf", watermark);
        let ctx = ToolCtx::default();

        let unknown_err = run_tool(
            "pdf.does_not_exist",
            std::slice::from_ref(&input),
            &json!({}),
            &ctx,
        )
        .unwrap_err();
        assert!(
            matches!(unknown_err, PdfError::InvalidParams(ref msg) if msg.contains("unknown tool")),
            "expected unknown tool error, got {unknown_err:?}"
        );

        let single_input_cases = [
            ("pdf.split", json!({"mode": "every", "every": 10})),
            ("pdf.extract_pages", json!({"pages": "1-3"})),
            ("pdf.delete_pages", json!({"pages": "2"})),
            ("pdf.reorder", json!({"order": "reverse"})),
            ("pdf.insert_blank", json!({"after": "1"})),
            ("pdf.rotate", json!({"angle": 90})),
            (
                "pdf.crop",
                json!({"box": {"left": 0.0, "bottom": 0.0, "right": 200.0, "top": 200.0}}),
            ),
            ("pdf.scale", json!({})),
            ("pdf.single_page", json!({})),
            ("pdf.rotate_content", json!({"angle_deg": 45.0})),
            ("pdf.decompress", json!({})),
            ("pdf.nup", json!({})),
            ("pdf.booklet", json!({})),
            ("pdf.divide_pages", json!({})),
            ("pdf.posterize", json!({})),
            ("pdf.metadata", json!({"set": {"title": "Updated"}})),
            ("pdf.outline", json!({"action": "clear"})),
            ("pdf.attachments", json!({"action": "list"})),
            (
                "pdf.page_labels",
                json!({"ranges": [{"from": 1, "style": "decimal"}]}),
            ),
            ("pdf.viewer_prefs", json!({"fit_window": true})),
            ("pdf.portfolio", json!({"action": "list"})),
        ];

        let two_input_cases = [
            ("pdf.merge", json!({})),
            ("pdf.alternate_mix", json!({})),
            ("pdf.overlay", json!({})),
        ];

        assert_eq!(single_input_cases.len() + two_input_cases.len() + 1, 25);

        for (tool_id, params) in single_input_cases {
            let out = run_tool(tool_id, std::slice::from_ref(&input2), &params, &ctx)
                .unwrap_or_else(|e| panic!("expected {tool_id} to succeed, got {e:?}"));
            assert!(
                !out.artifacts.is_empty(),
                "tool {tool_id} must produce at least 1 artifact"
            );
        }

        let two_inputs = [input.clone(), input2.clone()];
        for (tool_id, params) in two_input_cases {
            let out = run_tool(tool_id, &two_inputs, &params, &ctx)
                .unwrap_or_else(|e| panic!("expected {tool_id} to succeed, got {e:?}"));
            assert!(
                !out.artifacts.is_empty(),
                "tool {tool_id} must produce at least 1 artifact"
            );
        }

        let layers_out = run_tool(
            "pdf.layers",
            std::slice::from_ref(&watermark_input),
            &json!({"action": "list"}),
            &ctx,
        )
        .expect("pdf.layers list must succeed");
        assert_eq!(layers_out.artifacts.len(), 1);
    }

    #[test]
    fn central_dispatcher_records_repaired_xref_warning_and_rewrites_broken_xref_validly() {
        let broken = fixture_bytes("broken-xref.pdf");
        let input = InputDoc::new("broken-xref.pdf", broken);
        let ctx = ToolCtx::default();

        // 1. Running `pdf.rotate` on broken-xref.pdf records `REPAIRED_XREF_WARNING` and rewrites a valid PDF.
        let rotate_out = run_tool(
            "pdf.rotate",
            std::slice::from_ref(&input),
            &json!({"angle": 90}),
            &ctx,
        )
        .expect("pdf.rotate succeeds on broken-xref.pdf");
        assert_eq!(rotate_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
        assert_eq!(rotate_out.artifacts.len(), 1);
        assert!(
            core_pdf::read(&rotate_out.artifacts[0].bytes, &Limits::local()).is_ok(),
            "saved artifact from broken-xref.pdf must be a strictly valid PDF"
        );

        // 2. Central `dispatch_structural` with a passthrough runner proves `REPAIRED_XREF_WARNING`
        //    is populated automatically and the saved output is a strictly valid rewritten PDF.
        let output = dispatch_structural::<RotateParams, _>(
            "pdf.rotate",
            std::slice::from_ref(&input),
            &json!({"angle": 90}),
            &ctx,
            |docs, _params, _ctx| Ok(vec![OutputDoc::unnamed_pdf(docs[0].clone())]),
        )
        .expect("central dispatch_structural succeeds with passthrough output");

        assert_eq!(
            output.warnings,
            vec![REPAIRED_XREF_WARNING.to_owned()],
            "central dispatcher must always inject REPAIRED_XREF_WARNING when any input had repaired_xref"
        );
        assert_eq!(output.artifacts.len(), 1);
        assert_eq!(output.artifacts[0].name, "broken-xref.rotated.pdf");
        assert_eq!(output.artifacts[0].format, "pdf");
        assert!(
            core_pdf::read(&output.artifacts[0].bytes, &Limits::local()).is_ok(),
            "saved artifact from broken-xref.pdf must be a strictly valid PDF"
        );
    }

    #[test]
    fn encrypted_input_maps_to_pdf_error_encrypted_with_decrypt_hint() {
        use lopdf::{
            Document, EncryptionState, EncryptionVersion, Permissions,
            encryption::crypt_filters::{Aes256CryptFilter, CryptFilter},
        };
        use std::{collections::BTreeMap, sync::Arc};

        let plain = fixture_bytes("encrypted-owner-only.plaintext.pdf");
        let mut doc = Document::load_mem(&plain).expect("load plaintext");
        let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
        let crypt_filters = BTreeMap::from([(b"StdCF".to_vec(), filter)]);
        let key = [0x42_u8; 32];
        let version = EncryptionVersion::V5 {
            encrypt_metadata: true,
            crypt_filters,
            file_encryption_key: &key,
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "",
            permissions: Permissions::empty(),
        };
        let state = EncryptionState::try_from(version).expect("encryption state");
        doc.encrypt(&state).expect("encrypt doc");
        let mut enc_bytes = Vec::new();
        doc.save_to(&mut enc_bytes).expect("save encrypted");

        let input = InputDoc::new("encrypted.pdf", enc_bytes);
        let err = run_tool(
            "pdf.rotate",
            &[input],
            &json!({"angle": 90}),
            &ToolCtx::default(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            PdfError::Encrypted {
                needs_password: false
            }
        );
        assert!(encrypted_error_message(false).contains("pdf.decrypt"));
        assert!(encrypted_error_message(true).contains("pdf.decrypt"));
    }
}
