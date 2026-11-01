//! Implementation of the `pdf.attachments` tool and shared embedded-file helpers.

use std::collections::BTreeMap;

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::attachments::{AttachmentsAction, AttachmentsParams},
};
use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat, dictionary};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::{
        OutputDoc,
        metadata::catalog_object_id,
        outline::{decode_pdf_object_string, encode_pdf_string},
    },
    writer,
};

/// Metadata and raw payload of an embedded file extracted from a PDF.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EmbeddedFileEntry {
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

/// Serializable summary of an embedded file (`pdf.attachments` `list` and `pdf.portfolio` `list`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AttachmentSummary {
    /// Embedded file name.
    pub name: String,
    /// MIME media type (e.g. `"text/csv"`, `"text/plain"`, `"application/pdf"`).
    pub media_type: String,
    /// Uncompressed payload size in bytes.
    pub size: usize,
    /// Lowercase hex SHA-256 digest of the uncompressed payload.
    pub sha256: String,
}

/// Lists, embeds, extracts, or removes embedded file attachments in a PDF document.
pub fn run(
    inputs: &[PdfDoc],
    params: &AttachmentsParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.is_empty() {
        return Err(PdfError::InvalidParams(
            "pdf.attachments requires at least 1 input PDF".to_owned(),
        ));
    }

    let first_stem = naming::doc_stem(&inputs[0]).unwrap_or_else(|| "document".to_owned());

    match params.action {
        AttachmentsAction::List => {
            if inputs.len() != 1 {
                return Err(PdfError::InvalidParams(format!(
                    "pdf.attachments `list` requires exactly 1 input PDF, got {}",
                    inputs.len()
                )));
            }
            if params.names.is_some() {
                return Err(PdfError::InvalidParams(
                    "pdf.attachments `names` must not be provided when `action` is `list`"
                        .to_owned(),
                ));
            }
            let entries = read_embedded_files(&inputs[0], ctx)?;
            let summaries = entries_to_summaries(&entries);
            let out_name = naming::single_output_name(&first_stem, "attachments", "json");
            let json_val: Value =
                serde_json::to_value(&summaries).map_err(|_| PdfError::Malformed)?;
            Ok(vec![OutputDoc::from_json(out_name, json_val)?])
        }
        AttachmentsAction::Extract => {
            if inputs.len() != 1 {
                return Err(PdfError::InvalidParams(format!(
                    "pdf.attachments `extract` requires exactly 1 input PDF, got {}",
                    inputs.len()
                )));
            }
            extract_embedded_files_outputs(&inputs[0], &first_stem, params.names.as_deref(), ctx)
        }
        AttachmentsAction::Add => {
            if inputs.len() < 2 {
                return Err(PdfError::InvalidParams(
                    "pdf.attachments `add` requires at least 2 inputs (target PDF and at least 1 file to attach)"
                        .to_owned(),
                ));
            }
            let attach_inputs = &inputs[1..];
            if let Some(names) = &params.names {
                if names.len() != attach_inputs.len() {
                    return Err(PdfError::InvalidParams(format!(
                        "pdf.attachments `names` length ({}) must match number of attached inputs ({})",
                        names.len(),
                        attach_inputs.len()
                    )));
                }
                for name in names {
                    if name.trim().is_empty() {
                        return Err(PdfError::InvalidParams(
                            "attachment name in `names` must not be empty".to_owned(),
                        ));
                    }
                }
            }

            let mut base_pdf = inputs[0].clone();
            let mut existing = read_embedded_files(&base_pdf, ctx)?;
            let new_files =
                collect_attachment_payloads(attach_inputs, params.names.as_deref(), ctx)?;
            merge_embedded_entries(&mut existing, new_files);
            write_embedded_files(base_pdf.inner_mut()?, &existing)?;

            Ok(vec![OutputDoc::from_pdf(
                naming::default_pdf_output_name(&first_stem, "pdf.attachments"),
                base_pdf,
            )])
        }
        AttachmentsAction::Remove => {
            if inputs.len() != 1 {
                return Err(PdfError::InvalidParams(format!(
                    "pdf.attachments `remove` requires exactly 1 input PDF, got {}",
                    inputs.len()
                )));
            }
            let mut out_pdf = inputs[0].clone();
            let existing = read_embedded_files(&out_pdf, ctx)?;

            if let Some(target_names) = &params.names {
                if target_names.is_empty() {
                    return Err(PdfError::InvalidParams(
                        "pdf.attachments `remove` `names` list must not be empty when provided"
                            .to_owned(),
                    ));
                }
                for name in target_names {
                    if !existing.iter().any(|e| &e.name == name) {
                        return Err(PdfError::InvalidParams(format!(
                            "attachment {name:?} not found in document"
                        )));
                    }
                }
                let remaining: Vec<EmbeddedFileEntry> = existing
                    .into_iter()
                    .filter(|e| !target_names.contains(&e.name))
                    .collect();
                write_embedded_files(out_pdf.inner_mut()?, &remaining)?;
            } else {
                write_embedded_files(out_pdf.inner_mut()?, &[])?;
            }

            Ok(vec![OutputDoc::from_pdf(
                naming::default_pdf_output_name(&first_stem, "pdf.attachments"),
                out_pdf,
            )])
        }
    }
}

pub(crate) fn max_attachment_bytes() -> u64 {
    Limits::cloud()
        .max_asset_bytes
        .or(Limits::local().max_asset_bytes)
        .unwrap_or(50 * 1024 * 1024)
}

pub(crate) fn entries_to_summaries(entries: &[EmbeddedFileEntry]) -> Vec<AttachmentSummary> {
    entries
        .iter()
        .map(|e| AttachmentSummary {
            name: e.name.clone(),
            media_type: e.media_type.clone(),
            size: e.bytes.len(),
            sha256: e.sha256.clone(),
        })
        .collect()
}

pub(crate) fn collect_attachment_payloads(
    attach_inputs: &[PdfDoc],
    explicit_names: Option<&[String]>,
    ctx: &ToolCtx,
) -> Result<Vec<EmbeddedFileEntry>, PdfError> {
    let limit = max_attachment_bytes();
    let mut out = Vec::with_capacity(attach_inputs.len());

    for (idx, input_pdf) in attach_inputs.iter().enumerate() {
        ctx.check()?;
        let raw_name = if let Some(names) = explicit_names {
            names[idx].trim().to_owned()
        } else {
            let stem =
                naming::doc_stem(input_pdf).unwrap_or_else(|| format!("attachment-{}", idx + 1));
            if stem.contains('.') {
                stem
            } else {
                format!("{stem}.pdf")
            }
        };

        let mut budget = input_pdf.decode_budget();
        let mut cloned = input_pdf.clone();
        let bytes = writer::save(&mut cloned, ctx)?;
        if (bytes.len() as u64) > limit {
            return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
                actual: bytes.len() as u64,
                limit,
            }));
        }
        budget.charge(bytes.len())?;
        let media_type = infer_media_type(&raw_name);
        let sha256 = sha256_hex(&bytes);
        out.push(EmbeddedFileEntry {
            name: raw_name,
            media_type,
            bytes,
            sha256,
        });
    }

    Ok(out)
}

pub(crate) fn merge_embedded_entries(
    existing: &mut Vec<EmbeddedFileEntry>,
    new_entries: Vec<EmbeddedFileEntry>,
) {
    let mut by_name: BTreeMap<String, EmbeddedFileEntry> =
        existing.drain(..).map(|e| (e.name.clone(), e)).collect();
    for entry in new_entries {
        by_name.insert(entry.name.clone(), entry);
    }
    *existing = by_name.into_values().collect();
}

pub(crate) fn extract_embedded_files_outputs(
    pdf: &PdfDoc,
    stem: &str,
    filter_names: Option<&[String]>,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let entries = read_embedded_files(pdf, ctx)?;
    let selected: Vec<&EmbeddedFileEntry> = if let Some(names) = filter_names {
        if names.is_empty() {
            return Err(PdfError::InvalidParams(
                "`names` filter must not be empty when provided".to_owned(),
            ));
        }
        for requested in names {
            if !entries.iter().any(|e| &e.name == requested) {
                return Err(PdfError::InvalidParams(format!(
                    "attachment {requested:?} not found in document"
                )));
            }
        }
        entries.iter().filter(|e| names.contains(&e.name)).collect()
    } else {
        entries.iter().collect()
    };

    let clean_stem = naming::sanitize_stem(stem);
    let total = selected.len();
    let width = total.max(1).to_string().len().max(3);
    let mut outputs = Vec::with_capacity(total);
    for (idx, entry) in selected.iter().enumerate() {
        let index_1 = idx + 1;
        let clean_name = sanitize_attachment_filename(&entry.name);
        let out_name = format!("{clean_stem}.attachment-{index_1:0width$}.{clean_name}");
        let format = extension_from_filename(&entry.name);
        outputs.push(OutputDoc::from_bytes(out_name, format, entry.bytes.clone()));
    }
    Ok(outputs)
}

fn sanitize_attachment_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let filtered: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = filtered.trim_matches('.');
    if trimmed.is_empty() || trimmed.contains("..") {
        "attachment.bin".to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(crate) fn read_embedded_files(
    pdf: &PdfDoc,
    ctx: &ToolCtx,
) -> Result<Vec<EmbeddedFileEntry>, PdfError> {
    let doc = pdf.inner();
    let mut budget = pdf.decode_budget();
    let catalog_id = catalog_object_id(doc)?;
    let Ok(Object::Dictionary(catalog)) = doc.get_object(catalog_id) else {
        return Ok(Vec::new());
    };

    let mut raw_specs: Vec<(Option<String>, ObjectId)> = Vec::new();
    if let Ok(names_obj) = catalog.get(b"Names")
        && let Some(names_dict) = resolve_dict(doc, names_obj)
        && let Ok(ef_obj) = names_dict.get(b"EmbeddedFiles")
        && let Some(ef_dict) = resolve_dict(doc, ef_obj)
    {
        collect_name_tree_filespecs(doc, ef_dict, &mut raw_specs);
    }

    if raw_specs.is_empty()
        && let Ok(af_obj) = catalog.get(b"AF")
        && let Some(Object::Array(af_arr)) = resolve_object(doc, af_obj)
    {
        for item in af_arr {
            if let Object::Reference(spec_id) = item {
                raw_specs.push((None, *spec_id));
            }
        }
    }

    let limit = max_attachment_bytes();
    let mut by_name: BTreeMap<String, EmbeddedFileEntry> = BTreeMap::new();

    for (tree_name, spec_id) in raw_specs {
        ctx.check()?;
        let Ok(Object::Dictionary(filespec)) = doc.get_object(spec_id) else {
            continue;
        };
        let name = filespec
            .get(b"UF")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .and_then(decode_pdf_object_string)
            .or_else(|| {
                filespec
                    .get(b"F")
                    .ok()
                    .and_then(|o| resolve_object(doc, o))
                    .and_then(decode_pdf_object_string)
            })
            .or(tree_name)
            .unwrap_or_else(|| format!("attachment-{}", spec_id.0));

        let Ok(ef_entry) = filespec.get(b"EF") else {
            continue;
        };
        let Some(ef_dict) = resolve_dict(doc, ef_entry) else {
            continue;
        };
        let stream_obj = ef_dict
            .get(b"UF")
            .or_else(|_| ef_dict.get(b"F"))
            .ok()
            .and_then(|o| resolve_object(doc, o));
        let Some(Object::Stream(stream)) = stream_obj else {
            continue;
        };

        if let Ok(params_obj) = stream.dict.get(b"Params")
            && let Some(params_dict) = resolve_dict(doc, params_obj)
            && let Ok(size_val) = params_dict.get(b"Size").and_then(Object::as_i64)
            && size_val >= 0
        {
            let declared = size_val as u64;
            if declared > limit {
                return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
                    actual: declared,
                    limit,
                }));
            }
        }

        if (stream.content.len() as u64) > limit {
            return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
                actual: stream.content.len() as u64,
                limit,
            }));
        }

        let bytes = match budget.decode(stream)? {
            Some(decoded) => decoded,
            None => {
                budget.charge(stream.content.len())?;
                stream.content.clone()
            }
        };
        if (bytes.len() as u64) > limit {
            return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
                actual: bytes.len() as u64,
                limit,
            }));
        }

        let media_type = stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .and_then(decode_pdf_object_string)
            .map(|s| s.replace("#2F", "/").replace("#2f", "/"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| infer_media_type(&name));

        let sha256 = sha256_hex(&bytes);
        by_name.insert(
            name.clone(),
            EmbeddedFileEntry {
                name,
                media_type,
                bytes,
                sha256,
            },
        );
    }

    Ok(by_name.into_values().collect())
}

fn collect_name_tree_filespecs(
    doc: &lopdf::Document,
    node: &Dictionary,
    out: &mut Vec<(Option<String>, ObjectId)>,
) {
    if let Ok(names_obj) = node.get(b"Names")
        && let Some(Object::Array(arr)) = resolve_object(doc, names_obj)
    {
        for [k_obj, v_obj] in arr.as_chunks::<2>().0 {
            let key = resolve_object(doc, k_obj).and_then(decode_pdf_object_string);
            if let Object::Reference(spec_id) = *v_obj {
                out.push((key, spec_id));
            }
        }
    }
    if let Ok(kids_obj) = node.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(doc, kids_obj)
    {
        for kid in kids {
            if let Some(kid_dict) = resolve_dict(doc, kid) {
                collect_name_tree_filespecs(doc, kid_dict, out);
            }
        }
    }
}

pub(crate) fn write_embedded_files(
    doc: &mut lopdf::Document,
    entries: &[EmbeddedFileEntry],
) -> Result<(), PdfError> {
    let catalog_id = catalog_object_id(doc)?;

    if entries.is_empty() {
        let names_ref = doc
            .get_object(catalog_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Names").ok())
            .cloned();

        if let Some(Object::Reference(names_id)) = names_ref {
            let remove_names_dict =
                if let Ok(Object::Dictionary(names_dict)) = doc.get_object_mut(names_id) {
                    names_dict.remove(b"EmbeddedFiles");
                    names_dict.is_empty()
                } else {
                    false
                };
            if remove_names_dict
                && let Ok(Object::Dictionary(catalog)) = doc.get_object_mut(catalog_id)
            {
                catalog.remove(b"Names");
            }
        } else if let Ok(Object::Dictionary(catalog)) = doc.get_object_mut(catalog_id)
            && let Ok(Object::Dictionary(names_dict)) = catalog.get_mut(b"Names")
        {
            names_dict.remove(b"EmbeddedFiles");
            if names_dict.is_empty() {
                catalog.remove(b"Names");
            }
        }

        if let Ok(Object::Dictionary(catalog)) = doc.get_object_mut(catalog_id) {
            catalog.remove(b"AF");
        }
        return Ok(());
    }

    let mut names_array: Vec<Object> = Vec::with_capacity(entries.len() * 2);
    let mut af_array: Vec<Object> = Vec::with_capacity(entries.len());

    for entry in entries {
        let subtype_escaped = entry.media_type.replace('/', "#2F");
        let ef_stream = Stream::new(
            dictionary! {
                "Type" => "EmbeddedFile",
                "Subtype" => Object::Name(subtype_escaped.into_bytes()),
                "Params" => dictionary! {
                    "Size" => Object::Integer(entry.bytes.len() as i64),
                },
            },
            entry.bytes.clone(),
        );
        let stream_id = doc.add_object(Object::Stream(ef_stream));

        let mut filespec = Dictionary::new();
        filespec.set("Type", Object::Name(b"Filespec".to_vec()));
        filespec.set(
            "F",
            Object::String(entry.name.as_bytes().to_vec(), StringFormat::Literal),
        );
        filespec.set("UF", encode_pdf_string(&entry.name));
        filespec.set("AFRelationship", Object::Name(b"Unspecified".to_vec()));
        filespec.set(
            "EF",
            dictionary! {
                "F" => Object::Reference(stream_id),
                "UF" => Object::Reference(stream_id),
            },
        );
        let filespec_id = doc.add_object(Object::Dictionary(filespec));

        names_array.push(Object::String(
            entry.name.as_bytes().to_vec(),
            StringFormat::Literal,
        ));
        names_array.push(Object::Reference(filespec_id));
        af_array.push(Object::Reference(filespec_id));
    }

    let ef_tree_id = doc.add_object(Object::Dictionary(dictionary! {
        "Names" => Object::Array(names_array),
    }));

    let existing_names_id = doc
        .get_object(catalog_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Names").ok())
        .and_then(|o| o.as_reference().ok());

    if let Some(names_id) = existing_names_id
        && let Ok(Object::Dictionary(names_dict)) = doc.get_object_mut(names_id)
    {
        names_dict.set("EmbeddedFiles", Object::Reference(ef_tree_id));
    } else {
        let mut names_dict = Dictionary::new();
        if let Ok(Object::Dictionary(catalog)) = doc.get_object(catalog_id)
            && let Ok(Object::Dictionary(inline_names)) = catalog.get(b"Names")
        {
            names_dict = inline_names.clone();
        }
        names_dict.set("EmbeddedFiles", Object::Reference(ef_tree_id));
        let names_id = doc.add_object(Object::Dictionary(names_dict));
        if let Ok(Object::Dictionary(catalog)) = doc.get_object_mut(catalog_id) {
            catalog.set("Names", Object::Reference(names_id));
        }
    }

    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .as_dict_mut()
        .map_err(|_| PdfError::Malformed)?;
    catalog.set("AF", Object::Array(af_array));

    Ok(())
}

fn resolve_object<'a>(doc: &'a lopdf::Document, obj: &'a Object) -> Option<&'a Object> {
    match obj {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

fn resolve_dict<'a>(doc: &'a lopdf::Document, obj: &'a Object) -> Option<&'a Dictionary> {
    resolve_object(doc, obj)?.as_dict().ok()
}

pub(crate) fn infer_media_type(filename: &str) -> String {
    match extension_from_filename(filename).as_str() {
        "csv" => "text/csv".to_owned(),
        "txt" | "md" => "text/plain".to_owned(),
        "json" => "application/json".to_owned(),
        "pdf" => "application/pdf".to_owned(),
        "html" | "htm" => "text/html".to_owned(),
        "xml" => "application/xml".to_owned(),
        "png" => "image/png".to_owned(),
        "jpg" | "jpeg" => "image/jpeg".to_owned(),
        _ => "application/octet-stream".to_owned(),
    }
}

pub(crate) fn extension_from_filename(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
        .unwrap_or_else(|| "bin".to_owned())
}

/// Pure-Rust FIPS 180-4 SHA-256 hex digest computation.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut hex = String::with_capacity(64);
    for word in h {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{word:08x}");
    }
    hex
}
