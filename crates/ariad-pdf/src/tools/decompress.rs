//! Implementation of the `pdf.decompress` tool.
//!
//! Decompresses PDF streams into uncompressed plaintext streams while enforcing the per-document
//! [`crate::document::DecodeBudget`] (`Limits::max_decompressed_bytes`) and expanding compressed
//! `/ObjStm` object streams into top-level indirect objects.

use std::collections::{HashMap, HashSet, VecDeque};

use ariad_core::tools::{page_range::DuplicatePolicy, params::decompress::DecompressParams};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::{DecodeBudget, PdfDoc, ToolCtx},
    error::PdfError,
    gc,
    tools::{OutputDoc, rotate::collect_page_ids},
};

/// Decompresses PDF streams and expands object streams within decoded-size limits.
pub fn run(
    inputs: &[PdfDoc],
    params: &DecompressParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.decompress requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }

    let mut doc = inputs[0].clone();
    let mut budget = doc.decode_budget();
    let page_ids = collect_page_ids(doc.inner(), ctx)?;
    let page_count = u32::try_from(page_ids.len()).unwrap_or(u32::MAX);

    let selected_page_ids: Option<Vec<ObjectId>> = match params.pages {
        Some(ref range) => {
            let pages = range
                .expand(page_count, DuplicatePolicy::Reject)
                .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
            Some(
                pages
                    .into_iter()
                    .map(|p| page_ids[(p - 1) as usize])
                    .collect(),
            )
        }
        None => None,
    };

    let inner = doc.inner_mut()?;

    // Prune unreachable orphan objects and obsolete `/ObjStm` / `/XRef` streams that `lopdf`
    // already expanded during bounded loading.
    gc::collect_document(inner, ctx)?;

    // Charge page content stream references (including repeated references in `/Contents` arrays)
    // and cache decoded stream bytes per `ObjectId`.
    let mut decoded_cache: HashMap<ObjectId, Vec<u8>> = HashMap::new();
    let pages_to_charge: &[ObjectId] = selected_page_ids.as_deref().unwrap_or(&page_ids);
    for &page_id in pages_to_charge {
        ctx.check()?;
        charge_and_decode_page_content_refs(inner, page_id, &mut budget, &mut decoded_cache, ctx)?;
    }

    let target_stream_ids: Vec<ObjectId> = match selected_page_ids {
        Some(ref sel_pages) => collect_page_reachable_streams(inner, sel_pages, ctx)?,
        None => inner
            .objects
            .iter()
            .filter_map(|(&id, obj)| matches!(obj, Object::Stream(_)).then_some(id))
            .collect(),
    };

    for stream_id in target_stream_ids {
        ctx.check()?;
        let decoded_opt = if let Some(cached) = decoded_cache.remove(&stream_id) {
            Some(cached)
        } else if let Some(Object::Stream(stream)) = inner.objects.get(&stream_id) {
            budget.decode(stream)?
        } else {
            None
        };

        if let (Some(decoded_bytes), Some(Object::Stream(stream))) =
            (decoded_opt, inner.objects.get_mut(&stream_id))
        {
            stream.content = decoded_bytes;
            stream.dict.remove(b"Filter");
            stream.dict.remove(b"DecodeParms");
            stream.dict.remove(b"FFilter");
            stream.dict.remove(b"FDecodeParms");
            let len = i64::try_from(stream.content.len()).unwrap_or(i64::MAX);
            stream.dict.set("Length", Object::Integer(len));
        }
    }

    Ok(vec![OutputDoc::unnamed_pdf(doc)])
}

fn charge_and_decode_page_content_refs(
    doc: &Document,
    page_id: ObjectId,
    budget: &mut DecodeBudget,
    decoded_cache: &mut HashMap<ObjectId, Vec<u8>>,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let Ok(page_dict) = doc.get_dictionary(page_id) else {
        return Ok(());
    };
    let Ok(contents_obj) = page_dict.get(b"Contents") else {
        return Ok(());
    };

    let mut stream_ids = Vec::new();
    match contents_obj {
        Object::Reference(id) => match doc.get_object(*id) {
            Ok(Object::Array(arr)) => {
                for item in arr {
                    if let Ok(sid) = item.as_reference() {
                        stream_ids.push(sid);
                    }
                }
            }
            _ => stream_ids.push(*id),
        },
        Object::Array(arr) => {
            for item in arr {
                if let Ok(sid) = item.as_reference() {
                    stream_ids.push(sid);
                }
            }
        }
        Object::Stream(stream) => {
            budget.decode(stream)?;
        }
        _ => {}
    }

    for sid in stream_ids {
        ctx.check()?;
        if let Some(existing) = decoded_cache.get(&sid) {
            budget.charge(existing.len())?;
            continue;
        }
        if let Ok(Object::Stream(stream)) = doc.get_object(sid)
            && let Some(decoded) = budget.decode(stream)?
        {
            decoded_cache.insert(sid, decoded);
        }
    }
    Ok(())
}

fn collect_page_reachable_streams(
    doc: &Document,
    selected_pages: &[ObjectId],
    ctx: &ToolCtx,
) -> Result<Vec<ObjectId>, PdfError> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut streams = Vec::new();

    for &page_id in selected_pages {
        if visited.insert(page_id) {
            queue.push_back(page_id);
        }
    }

    let mut steps = 0_usize;
    while let Some(id) = queue.pop_front() {
        steps = steps.wrapping_add(1);
        if steps.is_multiple_of(64) {
            ctx.check()?;
        }
        let Some(object) = doc.objects.get(&id) else {
            continue;
        };
        match object {
            Object::Stream(stream) => {
                streams.push(id);
                enqueue_dict_refs(&stream.dict, &mut visited, &mut queue);
            }
            Object::Dictionary(dict) => {
                enqueue_dict_refs(dict, &mut visited, &mut queue);
            }
            Object::Array(items) => {
                for item in items {
                    enqueue_object_refs(item, &mut visited, &mut queue);
                }
            }
            Object::Reference(target) if visited.insert(*target) => {
                queue.push_back(*target);
            }
            _ => {}
        }
    }

    streams.sort_unstable();
    streams.dedup();
    Ok(streams)
}

fn enqueue_dict_refs(
    dict: &Dictionary,
    visited: &mut HashSet<ObjectId>,
    queue: &mut VecDeque<ObjectId>,
) {
    for (key, value) in dict.iter() {
        if key == b"Parent" || key == b"P" {
            continue;
        }
        enqueue_object_refs(value, visited, queue);
    }
}

fn enqueue_object_refs(
    object: &Object,
    visited: &mut HashSet<ObjectId>,
    queue: &mut VecDeque<ObjectId>,
) {
    match object {
        Object::Reference(id) if visited.insert(*id) => {
            queue.push_back(*id);
        }
        Object::Array(items) => {
            for item in items {
                enqueue_object_refs(item, visited, queue);
            }
        }
        Object::Dictionary(dict) => {
            enqueue_dict_refs(dict, visited, queue);
        }
        Object::Stream(stream) => {
            enqueue_dict_refs(&stream.dict, visited, queue);
        }
        _ => {}
    }
}
