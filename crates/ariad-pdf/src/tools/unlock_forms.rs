//! Clear read-only flags in AcroForm fields.

use std::collections::{HashMap, HashSet};

use ariad_core::{
    limits::Limits, reader::pdf::ReadError, tools::params::unlock_forms::UnlockFormsParams,
};
use lopdf::{Document, Object, ObjectId};
use serde_json::json;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::{InputDoc, OutputDoc, ToolWarning, metadata::catalog_object_id},
    writer::SaveOptions,
};

/// Unlocks non-signature field flags, retaining the configured traversal limits.
pub fn run_inputs(
    inputs: &[InputDoc],
    _params: &UnlockFormsParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.unlock_forms requires exactly one input PDF".into(),
        ));
    }
    let input = inputs[0].open_for_mutation(ctx)?;
    let mut result = input.clone();
    let limits = &inputs[0].limits;
    let doc = result.inner_mut()?;
    let root = catalog_object_id(doc)?;
    let form = doc
        .get_object(root)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"AcroForm").ok())
        .cloned();
    let mut state = State {
        visited: HashSet::new(),
        unlocked: 0,
        protected_parents: 0,
        signature_subtrees: HashMap::new(),
        signature: false,
        xfa: false,
        limits,
        remaining: limits.max_blocks,
        ctx,
    };
    if let Some(mut form) = form {
        edit_form(doc, &mut form, &mut state, 0)?;
        if let Ok(catalog) = doc.get_object_mut(root).and_then(Object::as_dict_mut) {
            catalog.set("AcroForm", form);
        }
    }
    let mut out = OutputDoc::unnamed_pdf(result).with_save_options(SaveOptions {
        preserve_info_dates: true,
        omit_info_if_absent: true,
        ..SaveOptions::default()
    });
    out.metrics
        .insert("unlocked_fields".into(), json!(state.unlocked));
    out.metrics.insert(
        "read_only_parents_kept".into(),
        json!(state.protected_parents),
    );
    if state.protected_parents > 0 {
        out.coded_warnings.push(ToolWarning {
            code: "read_only_parent_kept".into(),
            message: "Read-only parent fields were kept to protect signature descendants.".into(),
        });
    }
    if state.signature {
        out.coded_warnings.push(ToolWarning {
            code: "signature_field_kept".into(),
            message: "Signature fields were kept unchanged.".into(),
        });
    }
    if state.xfa {
        out.coded_warnings.push(ToolWarning {
            code: "xfa_not_changed".into(),
            message: "XFA read-only state was not changed.".into(),
        });
    }
    Ok((vec![input], vec![out]))
}

struct State<'a> {
    visited: HashSet<ObjectId>,
    unlocked: u64,
    protected_parents: u64,
    signature_subtrees: HashMap<ObjectId, bool>,
    signature: bool,
    xfa: bool,
    limits: &'a Limits,
    remaining: u32,
    ctx: &'a ToolCtx,
}

impl State<'_> {
    fn check(&mut self, depth: u16) -> Result<(), PdfError> {
        self.ctx.check()?;
        if depth > self.limits.max_nesting_depth {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth,
            }));
        }
        if self.remaining == 0 {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: self.limits.max_blocks,
            }));
        }
        self.remaining -= 1;
        Ok(())
    }
}

fn edit_form(
    doc: &mut Document,
    form: &mut Object,
    state: &mut State<'_>,
    depth: u16,
) -> Result<(), PdfError> {
    state.check(depth)?;
    if let Object::Reference(id) = form {
        let id = *id;
        if !state.visited.insert(id) {
            return Err(PdfError::Malformed);
        }
        let mut value = doc.objects.get(&id).cloned().ok_or(PdfError::Malformed)?;
        edit_form(doc, &mut value, state, depth + 1)?;
        doc.objects.insert(id, value);
    } else if let Object::Dictionary(dict) = form {
        state.xfa = dict.has(b"XFA");
        if let Ok(fields) = dict.get_mut(b"Fields") {
            scan_signatures(doc, fields, 0, false, state)?;
            state.visited.clear();
            edit_fields(doc, fields, 0, false, 0, 0, state)?;
        }
    } else {
        return Err(PdfError::Malformed);
    }
    Ok(())
}

fn edit_fields(
    doc: &mut Document,
    obj: &mut Object,
    depth: u16,
    signature: bool,
    inherited: i64,
    output_inherited: i64,
    state: &mut State<'_>,
) -> Result<(), PdfError> {
    state.check(depth)?;
    match obj {
        Object::Reference(id) => {
            let id = *id;
            if !state.visited.insert(id) {
                return Err(PdfError::Malformed);
            }
            let mut value = doc.objects.get(&id).cloned().ok_or(PdfError::Malformed)?;
            edit_fields(
                doc,
                &mut value,
                depth + 1,
                signature,
                inherited,
                output_inherited,
                state,
            )?;
            doc.objects.insert(id, value);
        }
        Object::Array(fields) => {
            for field in fields {
                edit_fields(
                    doc,
                    field,
                    depth + 1,
                    signature,
                    inherited,
                    output_inherited,
                    state,
                )?;
            }
        }
        Object::Dictionary(field) => {
            let signature = match field.get(b"FT") {
                Ok(value) => {
                    resolve(doc, value, state.limits.max_nesting_depth)?
                        .as_name()
                        .map_err(|_| PdfError::Malformed)?
                        == b"Sig"
                }
                Err(_) => signature,
            };
            if signature {
                state.signature = true;
                return Ok(());
            }
            let flags = match field.get(b"Ff") {
                Ok(value) => resolve(doc, value, state.limits.max_nesting_depth)?
                    .as_i64()
                    .map_err(|_| PdfError::Malformed)?,
                Err(_) => inherited,
            };
            let has_own_flags = field.has(b"Ff");
            let protects_signature = if let Ok(kids) = field.get(b"Kids") {
                subtree_has_signature(doc, kids, depth.saturating_add(1), signature, state)?
            } else {
                false
            };
            let mut output_flags = if has_own_flags {
                flags
            } else {
                output_inherited
            };
            if protects_signature {
                if output_flags & 1 != 0 {
                    state.protected_parents += 1;
                }
            } else if flags & 1 != 0 || output_flags & 1 != 0 {
                output_flags = flags & !1;
                if has_own_flags || output_inherited != output_flags {
                    field.set("Ff", output_flags);
                    state.unlocked += 1;
                }
            }
            if let Ok(kids) = field.get_mut(b"Kids") {
                edit_fields(
                    doc,
                    kids,
                    depth.saturating_add(1),
                    signature,
                    flags,
                    output_flags,
                    state,
                )?;
            }
        }
        _ => return Err(PdfError::Malformed),
    }
    Ok(())
}

fn scan_signatures(
    doc: &Document,
    obj: &Object,
    depth: u16,
    signature: bool,
    state: &mut State<'_>,
) -> Result<bool, PdfError> {
    state.check(depth)?;
    match obj {
        Object::Reference(id) => {
            if !state.visited.insert(*id) {
                return Err(PdfError::Malformed);
            }
            let value = doc.objects.get(id).ok_or(PdfError::Malformed)?;
            let result = scan_signatures(doc, value, depth + 1, signature, state)?;
            state.signature_subtrees.insert(*id, result);
            Ok(result)
        }
        Object::Array(items) => {
            let mut found = false;
            for item in items {
                found |= scan_signatures(doc, item, depth + 1, signature, state)?;
            }
            Ok(found)
        }
        Object::Dictionary(field) => {
            let signature = match field.get(b"FT") {
                Ok(value) => {
                    resolve(doc, value, state.limits.max_nesting_depth)?
                        .as_name()
                        .map_err(|_| PdfError::Malformed)?
                        == b"Sig"
                }
                Err(_) => signature,
            };
            let children = match field.get(b"Kids") {
                Ok(kids) => scan_signatures(doc, kids, depth + 1, signature, state)?,
                Err(_) => false,
            };
            Ok(signature || children)
        }
        _ => Err(PdfError::Malformed),
    }
}

fn subtree_has_signature(
    doc: &Document,
    obj: &Object,
    depth: u16,
    signature: bool,
    state: &mut State<'_>,
) -> Result<bool, PdfError> {
    state.check(depth)?;
    match obj {
        Object::Reference(id) => state
            .signature_subtrees
            .get(id)
            .copied()
            .ok_or(PdfError::Malformed),
        Object::Array(items) => {
            let mut found = false;
            for item in items {
                found |=
                    subtree_has_signature(doc, item, depth.saturating_add(1), signature, state)?;
            }
            Ok(found)
        }
        Object::Dictionary(field) => {
            let signature = match field.get(b"FT") {
                Ok(value) => {
                    resolve(doc, value, state.limits.max_nesting_depth)?
                        .as_name()
                        .map_err(|_| PdfError::Malformed)?
                        == b"Sig"
                }
                Err(_) => signature,
            };
            Ok(signature
                || match field.get(b"Kids") {
                    Ok(kids) => {
                        subtree_has_signature(doc, kids, depth.saturating_add(1), signature, state)?
                    }
                    Err(_) => false,
                })
        }
        _ => Err(PdfError::Malformed),
    }
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object, limit: u16) -> Result<&'a Object, PdfError> {
    let mut value = obj;
    let mut seen = HashSet::new();
    while let Object::Reference(id) = value {
        if seen.len() >= usize::from(limit) {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep { limit }));
        }
        if !seen.insert(*id) {
            return Err(PdfError::Malformed);
        }
        value = doc.objects.get(id).ok_or(PdfError::Malformed)?;
    }
    Ok(value)
}
