//! Tool runners for form-data interchange.

use std::collections::{HashMap, HashSet};

use ariad_core::{
    reader::pdf::ReadError,
    tools::{
        DuplicatePolicy,
        params::{
            form_model::{FieldInfo, FieldType, FormValue},
            forms::{FormsExportFormat, FormsExportParams, FormsImportParams},
        },
    },
};
use serde_json::json;

use crate::{
    PdfDoc, PdfError, ToolCtx,
    forms::{
        fill::{self, FillOptions},
        flatten, list,
    },
    interchange::{
        self, FormRecord,
        names::BatchNames,
        sniff::{self, DataFormat},
    },
    tools::{InputDoc, InputSource, OutputDoc, ToolWarning, rotate::collect_page_ids},
};

fn one_pdf(inputs: &[InputDoc], ctx: &ToolCtx) -> Result<PdfDoc, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams("export requires one PDF".into()));
    }
    inputs[0].open_for_mutation(ctx)
}
fn raw_payload(input: &InputDoc) -> Result<&[u8], PdfError> {
    let InputSource::Bytes(bytes) = &input.source else {
        return Err(PdfError::InvalidParams(
            "import data must be raw bytes".into(),
        ));
    };
    if let Some(limit) = input.limits.max_input_bytes
        && bytes.len() as u64 > limit
    {
        return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
            actual: bytes.len() as u64,
            limit,
        }));
    }
    if bytes.starts_with(b"%PDF-") {
        return Err(PdfError::InvalidParams(
            "import data must not be a PDF".into(),
        ));
    }
    Ok(bytes)
}
fn coded(out: &mut OutputDoc, warnings: impl IntoIterator<Item = String>, record: Option<usize>) {
    for warning in warnings {
        if let Some((code, message)) = warning.split_once(": ") {
            let message =
                record.map_or_else(|| message.to_owned(), |n| format!("record {n}: {message}"));
            out.coded_warnings.push(ToolWarning {
                code: code.into(),
                message,
            });
        } else {
            out.warnings.push(warning);
        }
    }
}
fn fields(
    pdf: &PdfDoc,
    inputs: &[InputDoc],
    pages: &[u32],
    ctx: &ToolCtx,
) -> Result<Vec<FieldInfo>, PdfError> {
    Ok(list::list(pdf, pages, true, &inputs[0].limits, ctx)?.fields)
}
fn all_pages(pdf: &PdfDoc, ctx: &ToolCtx) -> Result<Vec<u32>, PdfError> {
    let count = u32::try_from(collect_page_ids(pdf.inner(), ctx)?.len())
        .map_err(|_| PdfError::Malformed)?;
    Ok((1..=count).collect())
}
fn checkbox_value(value: &mut FormValue) {
    if let FormValue::Text(s) = value {
        match s.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => *value = FormValue::Bool(true),
            "false" | "no" | "off" | "0" => *value = FormValue::Bool(false),
            _ => {}
        }
    }
}

/// Import one or more form-data records and emit one PDF per record.
pub fn forms_import(
    inputs: &[InputDoc],
    params: &FormsImportParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    params.validate().map_err(PdfError::InvalidParams)?;
    if inputs.len() != 2 {
        return Err(PdfError::InvalidParams(
            "form import requires a PDF and one data file".into(),
        ));
    }
    let payload = raw_payload(&inputs[1])?;
    let format = sniff::sniff(payload)?;
    interchange::limits::check(payload, format, params.delimiter, &inputs[1].limits, ctx)?;
    let (mut records, parse_warnings) = match format {
        DataFormat::Json => interchange::json::read_forms(payload, params.max_records, ctx)?,
        DataFormat::Csv => (
            interchange::csv::read_forms(
                payload,
                params.delimiter,
                params.empty,
                params.max_records,
                ctx,
            )?,
            Vec::new(),
        ),
        DataFormat::Fdf => (
            vec![interchange::fdf::read_forms(payload, ctx)?],
            Vec::new(),
        ),
        DataFormat::Xfdf => {
            let (record, warnings) = interchange::xfdf::read_forms(payload, ctx)?;
            (vec![record], warnings)
        }
    };
    if records.is_empty() || records.len() > params.max_records {
        return Err(PdfError::InvalidParams(
            "invalid number of import records".into(),
        ));
    }
    let input_bytes = match &inputs[0].source {
        InputSource::Bytes(bytes) => bytes.len() as u64,
        InputSource::Doc(doc) => doc.source_len(),
    };
    let byte_limit = inputs[0].limits.max_decompressed_bytes;
    if input_bytes.saturating_mul(records.len() as u64) > byte_limit {
        return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
            limit: byte_limit,
        }));
    }
    let pdf = inputs[0].open_for_mutation(ctx)?;
    let pages = all_pages(&pdf, ctx)?;
    let mut metadata = HashMap::new();
    for field in fields(&pdf, inputs, &pages, ctx)? {
        metadata.entry(field.name.clone()).or_insert(field);
    }
    let total = records.len();
    let mut names = BatchNames::default();
    let mut outputs = Vec::with_capacity(total);
    let mut output_bytes = 0u64;
    let mut unknown_seen = HashSet::new();
    for (index, record) in records.iter_mut().enumerate() {
        ctx.check()?;
        let filename = params
            .name_column
            .as_ref()
            .and_then(|name| record.get(name))
            .and_then(|value| match value {
                FormValue::Text(s) => Some(s.clone()),
                _ => None,
            });
        if let Some(column) = &params.name_column
            && !metadata.contains_key(column)
        {
            record.remove(column);
        }
        let mut unknown_warnings = Vec::new();
        for (name, value) in record.iter_mut() {
            if let Some(field) = metadata.get(name) {
                if field.field_type == Some(FieldType::Checkbox) {
                    checkbox_value(value);
                }
                if format == DataFormat::Csv
                    && field.field_type == Some(FieldType::List)
                    && field.flags.multi_select
                    && let FormValue::Text(text) = value
                {
                    *value = FormValue::Multi(text.split('|').map(str::to_owned).collect());
                }
            } else if unknown_seen.insert(name.clone()) {
                unknown_warnings.push("unknown_field: field was not found".to_owned());
            }
        }
        if !unknown_warnings.is_empty() && params.strict {
            return Err(PdfError::InvalidParams(
                "unknown form field in import".into(),
            ));
        }
        record.retain(|name, _| metadata.contains_key(name));
        let mut result = pdf.clone();
        let filled = fill::fill(
            &mut result,
            record,
            &FillOptions {
                strict: params.strict,
            },
            &inputs[0].limits,
            ctx,
        )?;
        let flattened = if params.flatten && !filled.affected.is_empty() {
            Some(flatten::flatten(
                &mut result,
                &pages,
                Some(&filled.affected),
                &inputs[0].limits,
                ctx,
            )?)
        } else {
            None
        };
        let bytes = crate::writer::save(&mut result, ctx)?;
        drop(result);
        output_bytes = output_bytes.saturating_add(bytes.len() as u64);
        if output_bytes > byte_limit {
            return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
                limit: byte_limit,
            }));
        }
        let name = if total == 1 {
            String::new()
        } else {
            names.allocate(filename.as_deref(), &inputs[0].stem, index + 1, total)
        };
        let mut out = OutputDoc::from_bytes(name, "pdf", bytes);
        if index == 0 {
            coded(&mut out, parse_warnings.clone(), None);
        }
        coded(&mut out, unknown_warnings, Some(index + 1));
        coded(&mut out, filled.warnings, Some(index + 1));
        if let Some(flattened) = flattened {
            coded(&mut out, flattened.warnings, Some(index + 1));
        }
        out.metrics.insert("record".into(), json!(index + 1));
        outputs.push(out);
    }
    Ok((vec![pdf], outputs))
}

/// Export selected field values to the requested interchange format.
pub fn forms_export(
    inputs: &[InputDoc],
    params: &FormsExportParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    let pdf = one_pdf(inputs, ctx)?;
    let count = u32::try_from(collect_page_ids(pdf.inner(), ctx)?.len())
        .map_err(|_| PdfError::Malformed)?;
    let pages = params
        .pages
        .expand(count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let field_list = fields(&pdf, inputs, &pages, ctx)?;
    let mut record = FormRecord::new();
    let mut state_record = FormRecord::new();
    let mut state_fields = HashSet::new();
    for field in field_list {
        ctx.check()?;
        if matches!(
            field.field_type,
            Some(FieldType::Button | FieldType::Signature) | None
        ) {
            continue;
        }
        let mut state = field.value.clone();
        if field.field_type == Some(FieldType::Checkbox) {
            state = match field.value {
                FormValue::Bool(false) | FormValue::Null(()) => FormValue::Text("Off".into()),
                FormValue::Bool(true) => FormValue::Text(
                    field
                        .widgets
                        .iter()
                        .find_map(|w| w.on_state.clone())
                        .ok_or_else(|| {
                            PdfError::InvalidParams("checkbox has no usable on-state".into())
                        })?,
                ),
                ref value => value.clone(),
            };
        }
        if matches!(
            field.field_type,
            Some(FieldType::Checkbox | FieldType::Radio)
        ) {
            state_fields.insert(field.name.clone());
        }
        state_record.insert(field.name.clone(), state);
        record.insert(field.name, field.value);
    }
    let mut dropped_xml_chars = false;
    let (format, bytes) = match params.format {
        FormsExportFormat::Json => ("json", interchange::json::write_forms(&record)?),
        FormsExportFormat::Csv => ("csv", interchange::csv::write_forms(&record)?),
        FormsExportFormat::Fdf => (
            "fdf",
            interchange::fdf::write_forms_with_names(&state_record, &state_fields)?,
        ),
        FormsExportFormat::Xfdf => {
            let (bytes, dropped) =
                interchange::xfdf::write_forms_with_warnings(&state_record, &inputs[0].stem)?;
            dropped_xml_chars = dropped;
            ("xfdf", bytes)
        }
    };
    let mut output = if format == "json" {
        let value = serde_json::from_slice(&bytes).map_err(|_| PdfError::Malformed)?;
        OutputDoc::from_json(String::new(), value)?
    } else {
        OutputDoc::from_bytes(String::new(), format, bytes)
    };
    if dropped_xml_chars {
        coded(
            &mut output,
            ["invalid_xml_characters: XML-invalid characters were dropped".into()],
            None,
        );
    }
    Ok((vec![pdf], vec![output]))
}

/// Import annotations from XFDF or AriadShift JSON.
pub fn annotations_import(
    inputs: &[InputDoc],
    params: &ariad_core::tools::params::annotations::AnnotationsImportParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    if inputs.len() != 2 {
        return Err(PdfError::InvalidParams(
            "annotation import requires a PDF and one data file".into(),
        ));
    }
    let input = inputs[0].open_for_mutation(ctx)?;
    let payload = raw_payload(&inputs[1])?;
    let format = sniff::sniff(payload)?;
    interchange::limits::check(payload, format, None, &inputs[1].limits, ctx)?;
    let (items, parse_warnings) = match format {
        DataFormat::Json => (
            interchange::json::read_annotations(payload, ctx)?,
            Vec::new(),
        ),
        DataFormat::Xfdf => interchange::xfdf_annotations::read(payload, ctx)?,
        _ => {
            return Err(PdfError::InvalidParams(
                "annotation import accepts XFDF or JSON".into(),
            ));
        }
    };
    let mut pdf = input.clone();
    let mut warnings = parse_warnings;
    if params.replace {
        let selector = crate::annotations::AnnotationSelector {
            pages: all_pages(&pdf, ctx)?,
            ids: None,
            types: None,
            all: true,
        };
        warnings.extend(
            crate::annotations::modify::delete(&mut pdf, &selector, &inputs[0].limits, ctx)?
                .warnings,
        );
    }
    let count = items.len();
    if !items.is_empty() {
        warnings.extend(
            crate::annotations::write::add(&mut pdf, &items, &inputs[0].limits, ctx)?.warnings,
        );
    }
    let mut output = OutputDoc::unnamed_pdf(pdf);
    coded(&mut output, warnings, None);
    output
        .metrics
        .insert("affected_annotations".into(), json!(count));
    Ok((vec![input], vec![output]))
}

/// Export selected annotations as XFDF or JSON.
pub fn annotations_export(
    inputs: &[InputDoc],
    params: &ariad_core::tools::params::annotations::AnnotationsExportParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    use ariad_core::tools::params::annotations::AnnotationsExportFormat;
    let pdf = one_pdf(inputs, ctx)?;
    let count = u32::try_from(collect_page_ids(pdf.inner(), ctx)?.len())
        .map_err(|_| PdfError::Malformed)?;
    let pages = params
        .pages
        .expand(count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let selector = crate::annotations::AnnotationSelector {
        pages,
        ids: None,
        types: None,
        all: true,
    };
    let mut listing = crate::annotations::read::list(&pdf, &selector, &inputs[0].limits, ctx)?;
    if listing.truncated {
        return Err(PdfError::InvalidParams(
            "too many annotations to export".into(),
        ));
    }
    let original_count = listing.annotations.len();
    listing
        .annotations
        .retain(|info| info.to_item().is_some_and(|item| item.validate().is_ok()));
    if listing.annotations.len() != original_count {
        listing
            .warnings
            .push("annotation_type_skipped: unsupported annotations were omitted".into());
    }
    let mut output = match params.format {
        AnnotationsExportFormat::Json => OutputDoc::from_json(
            String::new(),
            serde_json::to_value(&listing).map_err(|_| PdfError::Malformed)?,
        )?,
        AnnotationsExportFormat::Xfdf => OutputDoc::from_bytes(
            String::new(),
            "xfdf",
            interchange::xfdf_annotations::write(&listing.annotations, &inputs[0].stem, ctx)?,
        ),
    };
    coded(&mut output, listing.warnings, None);
    Ok((vec![pdf], vec![output]))
}
