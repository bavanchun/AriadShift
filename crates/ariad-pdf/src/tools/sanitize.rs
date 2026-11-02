//! Remove selected active and privacy-sensitive PDF content.

use std::collections::HashSet;

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::{SanitizeForms, SanitizeParams},
};
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat, content::Content};
use serde_json::json;

use crate::{
    document::{DecodeBudget, PdfDoc, ToolCtx},
    error::PdfError,
    flatten,
    tools::{
        InputDoc, OutputDoc, ToolWarning, attachments,
        metadata::{catalog_object_id, remove_info_and_xmp},
    },
    writer::SaveOptions,
};

/// Sanitizes a single PDF using the caller's traversal limits.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &SanitizeParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.sanitize requires exactly one input PDF".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let input = inputs[0].open_for_mutation(ctx)?;
    let mut result = input.clone();
    let mut decode_budget = result.decode_budget();
    let limits = &inputs[0].limits;
    let doc = result.inner_mut()?;
    let pages = collect_pages(doc, limits, ctx)?;
    let mut stats = Stats::default();
    let mut budget = WalkBudget {
        remaining: limits.max_blocks,
        limits,
        ctx,
    };
    if params.javascript {
        escape_displayed_script_names(doc, &pages, &mut decode_budget, &mut budget)?;
    }
    if params.forms == SanitizeForms::Flatten {
        let report = flatten::flatten_appearances(
            doc,
            &pages,
            flatten::FlattenFilter {
                select: |d: &Dictionary| {
                    d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget")
                },
                limits,
            },
            ctx,
        )?;
        stats.flattened_widgets = report.flattened_count;
        stats.no_appearance = report.without_appearance > 0;
        stats.signature = report.signature_invalidated;
        // Field-tree pruning must keep fields whose widgets were not flattened.
        prune_fields(
            doc,
            &report.flattened,
            &report.flattened_direct,
            limits,
            ctx,
        )?;
    }
    if params.embedded_files {
        let root = catalog_object_id(doc)?;
        let catalog = doc
            .get_object(root)
            .and_then(Object::as_dict)
            .map_err(|_| PdfError::Malformed)?;
        if let Ok(names) = catalog.get(b"Names")
            && deref(doc, names, limits)?
                .as_dict()
                .map_err(|_| PdfError::Malformed)?
                .has(b"EmbeddedFiles")
        {
            stats.embedded += 1;
        }
        if catalog.has(b"AF") {
            stats.embedded += 1;
        }
        attachments::write_embedded_files(doc, &[])?;
    }
    if params.metadata {
        let root = catalog_object_id(doc)?;
        let info_object = doc
            .trailer
            .get(b"Info")
            .ok()
            .and_then(|value| value.as_reference().ok())
            .and_then(|id| doc.objects.get(&id).cloned().map(|object| (id, object)));
        let xmp_object = if params.xmp {
            doc.get_object(root)
                .and_then(Object::as_dict)
                .ok()
                .and_then(|catalog| catalog.get(b"Metadata").ok())
                .and_then(|value| value.as_reference().ok())
                .and_then(|id| doc.objects.get(&id).cloned().map(|object| (id, object)))
        } else {
            None
        };
        let saved_xmp = if !params.xmp {
            doc.get_object_mut(root)
                .and_then(Object::as_dict_mut)
                .map_err(|_| PdfError::Malformed)?
                .remove(b"Metadata")
        } else {
            None
        };
        remove_info_and_xmp(doc)?;
        // The writer removes unreferenced objects; retain shared objects until then.
        if let Some((id, object)) = info_object {
            doc.objects.insert(id, object);
        }
        if let Some((id, object)) = xmp_object {
            doc.objects.insert(id, object);
        }
        if let Some(xmp) = saved_xmp {
            doc.get_object_mut(root)
                .and_then(Object::as_dict_mut)
                .map_err(|_| PdfError::Malformed)?
                .set("Metadata", xmp);
        }
    }
    let snapshot = doc.clone();
    for (id, object) in &mut doc.objects {
        budget.check(0)?;
        let previous = (stats.javascript, stats.links);
        let action = object.as_dict().ok().is_some_and(|dict| dict.has(b"S"));
        let counted = action && stats.counted_actions.contains(id);
        scrub_object(object, &snapshot, params, &mut stats, &mut budget, 0)?;
        if counted {
            (stats.javascript, stats.links) = previous;
        } else if action && (stats.javascript, stats.links) != previous {
            stats.counted_actions.insert(*id);
        }
    }
    for page_id in pages {
        ctx.check()?;
        clean_page_annotations(doc, page_id, params, &mut stats, limits)?;
    }
    if params.forms == SanitizeForms::Remove {
        let root = catalog_object_id(doc)?;
        doc.get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .map_err(|_| PdfError::Malformed)?
            .remove(b"AcroForm");
    }
    let mut out = OutputDoc::unnamed_pdf(result).with_save_options(SaveOptions {
        preserve_info_dates: true,
        omit_info_if_absent: true,
        ..SaveOptions::default()
    });
    for (name, value) in [
        ("removed_javascript", stats.javascript),
        ("removed_embedded_files", stats.embedded),
        ("removed_links", stats.links),
        ("removed_annotations", stats.annotations),
        ("flattened_widgets", stats.flattened_widgets),
    ] {
        out.metrics.insert(name.into(), json!(value));
    }
    for (enabled, code, message) in [
        (stats.xfa, "xfa_removed", "XFA was removed."),
        (
            stats.no_appearance,
            "annotation_without_appearance",
            "An annotation had no usable appearance.",
        ),
        (
            stats.signature,
            "signature_invalidated",
            "A signature appearance was flattened.",
        ),
    ] {
        if enabled {
            out.coded_warnings.push(ToolWarning {
                code: code.into(),
                message: message.into(),
            });
        }
    }
    Ok((vec![input], vec![out]))
}

fn collect_pages(
    doc: &Document,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<ObjectId>, PdfError> {
    let catalog = doc
        .get_object(catalog_object_id(doc)?)
        .and_then(Object::as_dict)
        .map_err(|_| PdfError::Malformed)?;
    let root = catalog
        .get(b"Pages")
        .map_err(|_| PdfError::Malformed)?
        .clone();
    let mut stack = vec![(root, 0u16)];
    let mut seen = HashSet::new();
    let mut pages = Vec::new();
    let mut steps = 0u32;
    while let Some((object, depth)) = stack.pop() {
        ctx.check()?;
        if depth > limits.max_nesting_depth {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: limits.max_nesting_depth,
            }));
        }
        steps = steps.saturating_add(1);
        if steps > limits.max_blocks {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: limits.max_blocks,
            }));
        }
        match object {
            Object::Reference(id) => {
                if !seen.insert(id) {
                    return Err(PdfError::Malformed);
                }
                let value = doc.objects.get(&id).ok_or(PdfError::Malformed)?;
                if value
                    .as_dict()
                    .ok()
                    .and_then(|d| d.get(b"Type").and_then(Object::as_name).ok())
                    == Some(b"Page")
                {
                    pages.push(id);
                } else {
                    stack.push((value.clone(), depth + 1));
                }
            }
            Object::Dictionary(dict) => stack.push((
                dict.get(b"Kids").map_err(|_| PdfError::Malformed)?.clone(),
                depth + 1,
            )),
            Object::Array(kids) => {
                for kid in kids.into_iter().rev() {
                    stack.push((kid, depth + 1));
                }
            }
            _ => return Err(PdfError::Malformed),
        }
    }
    Ok(pages)
}

fn escape_displayed_script_names(
    doc: &mut Document,
    pages: &[ObjectId],
    decode_budget: &mut DecodeBudget,
    budget: &mut WalkBudget<'_>,
) -> Result<(), PdfError> {
    let mut seen = HashSet::new();
    for page in pages {
        budget.ctx.check()?;
        for id in doc.get_page_contents(*page) {
            if !seen.insert(id) {
                continue;
            }
            let stream = doc
                .get_object(id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?;
            let bytes = decode_budget.decode(stream)?.ok_or(PdfError::Malformed)?;
            if !bytes.windows(3).any(|part| part == b"/JS")
                && !bytes.windows(11).any(|part| part == b"/JavaScript")
            {
                continue;
            }
            let mut content = Content::decode(&bytes).map_err(|_| PdfError::Malformed)?;
            let mut changed = false;
            for operation in &mut content.operations {
                budget.check(0)?;
                for operand in &mut operation.operands {
                    changed |= hex_script_strings(operand, budget, 0)?;
                }
            }
            if changed {
                let bytes = content.encode().map_err(|_| PdfError::Malformed)?;
                doc.get_object_mut(id)
                    .and_then(Object::as_stream_mut)
                    .map_err(|_| PdfError::Malformed)?
                    .set_plain_content(bytes);
            }
        }
    }
    Ok(())
}

fn hex_script_strings(
    object: &mut Object,
    budget: &mut WalkBudget<'_>,
    depth: u16,
) -> Result<bool, PdfError> {
    budget.check(depth)?;
    match object {
        Object::String(bytes, format)
            if bytes.windows(3).any(|part| part == b"/JS")
                || bytes.windows(11).any(|part| part == b"/JavaScript") =>
        {
            *format = StringFormat::Hexadecimal;
            Ok(true)
        }
        Object::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= hex_script_strings(item, budget, depth + 1)?;
            }
            Ok(changed)
        }
        Object::Dictionary(dict) => {
            let mut changed = false;
            for (_, item) in dict.iter_mut() {
                changed |= hex_script_strings(item, budget, depth + 1)?;
            }
            Ok(changed)
        }
        _ => Ok(false),
    }
}

#[derive(Default)]
struct Stats {
    counted_actions: HashSet<ObjectId>,
    javascript: u64,
    embedded: u64,
    links: u64,
    annotations: u64,
    flattened_widgets: u64,
    xfa: bool,
    no_appearance: bool,
    signature: bool,
}
struct WalkBudget<'a> {
    remaining: u32,
    limits: &'a Limits,
    ctx: &'a ToolCtx,
}
impl WalkBudget<'_> {
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

fn scrub_object(
    object: &mut Object,
    source: &Document,
    params: &SanitizeParams,
    stats: &mut Stats,
    budget: &mut WalkBudget<'_>,
    depth: u16,
) -> Result<(), PdfError> {
    budget.check(depth)?;
    match object {
        Object::Dictionary(dict) => scrub_dict(dict, source, params, stats, budget, depth),
        Object::Stream(stream) => {
            scrub_dict(&mut stream.dict, source, params, stats, budget, depth)
        }
        Object::Array(array) => {
            for item in array {
                scrub_object(item, source, params, stats, budget, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn scrub_dict(
    dict: &mut Dictionary,
    source: &Document,
    params: &SanitizeParams,
    stats: &mut Stats,
    budget: &mut WalkBudget<'_>,
    depth: u16,
) -> Result<(), PdfError> {
    if params.javascript {
        let action_type = dict
            .get(b"S")
            .ok()
            .map(|o| deref(source, o, budget.limits))
            .transpose()?
            .and_then(|o| o.as_name().ok());
        if action_type == Some(b"JavaScript") {
            dict.remove(b"S");
            stats.javascript += 1;
        }
        for key in [b"AA".as_slice(), b"JavaScript", b"JS"] {
            if dict.remove(key).is_some() {
                stats.javascript += 1;
            }
        }
        if dict.remove(b"XFA").is_some() {
            stats.xfa = true;
        }
        if dict
            .get(b"CO")
            .ok()
            .and_then(|o| o.as_array().ok())
            .is_some_and(Vec::is_empty)
        {
            dict.remove(b"CO");
        }
    }
    if params.xmp {
        dict.remove(b"Metadata");
    }
    if params.embedded_files {
        if dict.remove(b"EmbeddedFiles").is_some() {
            stats.embedded += 1;
        }
        if dict.remove(b"AF").is_some() {
            stats.embedded += 1;
        }
    }
    if params.thumbnails && dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Page") {
        dict.remove(b"Thumb");
    }
    if params.javascript || params.links {
        if let Ok(value) = dict.get(b"AA").cloned() {
            let mut actions = deref(source, &value, budget.limits)?
                .as_dict()
                .map_err(|_| PdfError::Malformed)?
                .clone();
            let keys: Vec<_> = actions.iter().map(|(key, _)| key.clone()).collect();
            for key in keys {
                let action = actions.get(&key).map_err(|_| PdfError::Malformed)?.clone();
                match clean_action(
                    &action,
                    source,
                    params,
                    stats,
                    budget,
                    depth + 1,
                    &mut HashSet::new(),
                )? {
                    Some(value) => actions.set(key, value),
                    None => {
                        actions.remove(&key);
                    }
                }
            }
            if actions.is_empty() {
                dict.remove(b"AA");
            } else {
                dict.set("AA", actions);
            }
        }
        for key in [b"A".as_slice(), b"OpenAction"] {
            if let Ok(value) = dict.get(key).cloned() {
                let mut active = HashSet::new();
                if params.javascript
                    && key == b"OpenAction"
                    && contains_javascript(&value, source, budget, depth + 1, &mut active)?
                {
                    clean_action(
                        &value,
                        source,
                        params,
                        stats,
                        budget,
                        depth + 1,
                        &mut active,
                    )?;
                    dict.remove(key);
                    continue;
                }
                match clean_action(
                    &value,
                    source,
                    params,
                    stats,
                    budget,
                    depth + 1,
                    &mut active,
                )? {
                    Some(value) => dict.set(key, value),
                    None => {
                        dict.remove(key);
                    }
                }
            }
        }
    }
    for (_, value) in dict.iter_mut() {
        scrub_object(value, source, params, stats, budget, depth + 1)?;
    }
    Ok(())
}

fn contains_javascript(
    value: &Object,
    source: &Document,
    budget: &mut WalkBudget<'_>,
    depth: u16,
    active: &mut HashSet<ObjectId>,
) -> Result<bool, PdfError> {
    budget.check(depth)?;
    match value {
        Object::Reference(id) => {
            if !active.insert(*id) {
                return Err(PdfError::Malformed);
            }
            let result = contains_javascript(
                source.objects.get(id).ok_or(PdfError::Malformed)?,
                source,
                budget,
                depth + 1,
                active,
            )?;
            active.remove(id);
            Ok(result)
        }
        Object::Array(items) => {
            if items.get(1).is_some_and(|o| matches!(o, Object::Name(_))) {
                return Ok(false);
            }
            for item in items {
                if contains_javascript(item, source, budget, depth + 1, active)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Object::Dictionary(dict) => {
            let kind = dict
                .get(b"S")
                .ok()
                .map(|o| deref(source, o, budget.limits))
                .transpose()?
                .and_then(|o| o.as_name().ok());
            let js_uri = kind == Some(b"URI")
                && dict
                    .get(b"URI")
                    .ok()
                    .map(|value| deref(source, value, budget.limits))
                    .transpose()?
                    .and_then(|value| value.as_str().ok())
                    .is_some_and(|value| {
                        String::from_utf8_lossy(value)
                            .trim_start()
                            .to_ascii_lowercase()
                            .starts_with("javascript:")
                    });
            if matches!(
                kind,
                Some(b"JavaScript" | b"Launch" | b"SubmitForm" | b"ImportData")
            ) || kind == Some(b"Rendition") && dict.has(b"JS")
                || js_uri
            {
                return Ok(true);
            }
            match dict.get(b"Next") {
                Ok(next) => contains_javascript(next, source, budget, depth + 1, active),
                Err(_) => Ok(false),
            }
        }
        _ => Ok(false),
    }
}

fn clean_action(
    value: &Object,
    source: &Document,
    params: &SanitizeParams,
    stats: &mut Stats,
    budget: &mut WalkBudget<'_>,
    depth: u16,
    active: &mut HashSet<ObjectId>,
) -> Result<Option<Object>, PdfError> {
    budget.check(depth)?;
    match value {
        Object::Reference(id) => {
            if !active.insert(*id) {
                return Err(PdfError::Malformed);
            }
            let target = source.objects.get(id).ok_or(PdfError::Malformed)?;
            let previous = (stats.javascript, stats.links);
            let result = clean_action(target, source, params, stats, budget, depth + 1, active)?;
            if stats.counted_actions.contains(id) {
                (stats.javascript, stats.links) = previous;
            } else if (stats.javascript, stats.links) != previous {
                stats.counted_actions.insert(*id);
            }
            active.remove(id);
            Ok(result)
        }
        Object::Array(items) => {
            // Destination arrays are not action arrays.
            if items
                .first()
                .is_some_and(|o| !matches!(o, Object::Dictionary(_) | Object::Reference(_)))
                || items.len() > 1 && items.get(1).is_some_and(|o| matches!(o, Object::Name(_)))
            {
                return Ok(Some(value.clone()));
            }
            let mut kept = Vec::new();
            for item in items {
                if let Some(value) =
                    clean_action(item, source, params, stats, budget, depth + 1, active)?
                {
                    kept.push(value);
                }
            }
            Ok((!kept.is_empty()).then_some(Object::Array(kept)))
        }
        Object::Dictionary(dict) => {
            let action = match dict.get(b"S") {
                Ok(item) => deref(source, item, budget.limits)?.as_name().ok(),
                Err(_) => None,
            };
            if action.is_none() {
                return Ok(Some(value.clone()));
            }
            let js_uri = action == Some(b"URI")
                && dict
                    .get(b"URI")
                    .ok()
                    .map(|o| deref(source, o, budget.limits))
                    .transpose()?
                    .and_then(|o| o.as_str().ok())
                    .is_some_and(|s| {
                        String::from_utf8_lossy(s)
                            .trim_start()
                            .to_ascii_lowercase()
                            .starts_with("javascript:")
                    });
            let javascript = params.javascript
                && (action == Some(b"JavaScript")
                    || action == Some(b"Rendition") && dict.has(b"JS")
                    || js_uri);
            let risky = params.javascript
                && matches!(action, Some(b"Launch" | b"SubmitForm" | b"ImportData"));
            let link = params.links
                && matches!(
                    action,
                    Some(b"URI" | b"Launch" | b"GoToR" | b"GoToE" | b"SubmitForm" | b"ImportData")
                );
            let next = match dict.get(b"Next") {
                Ok(next) => clean_action(next, source, params, stats, budget, depth + 1, active)?,
                Err(_) => None,
            };
            if javascript || risky || link {
                if javascript || risky {
                    stats.javascript += 1;
                } else {
                    stats.links += 1;
                }
                return Ok(next);
            }
            let mut clean = dict.clone();
            if let Some(next) = next {
                clean.set("Next", next);
            } else {
                clean.remove(b"Next");
            }
            Ok(Some(Object::Dictionary(clean)))
        }
        _ => Ok(Some(value.clone())),
    }
}

fn clean_page_annotations(
    doc: &mut Document,
    page_id: ObjectId,
    params: &SanitizeParams,
    stats: &mut Stats,
    limits: &Limits,
) -> Result<(), PdfError> {
    let page = doc
        .get_object(page_id)
        .and_then(Object::as_dict)
        .map_err(|_| PdfError::Malformed)?;
    let Some(annots) = page.get(b"Annots").ok() else {
        return Ok(());
    };
    let entries = deref(doc, annots, limits)?
        .as_array()
        .map_err(|_| PdfError::Malformed)?
        .clone();
    let mut removed = HashSet::new();
    let mut kept = Vec::new();
    for entry in entries {
        let dict = deref(doc, &entry, limits)?
            .as_dict()
            .map_err(|_| PdfError::Malformed)?;
        let subtype = match dict.get(b"Subtype") {
            Ok(value) => deref(doc, value, limits)?.as_name().unwrap_or_default(),
            Err(_) => &[],
        };
        let remove = params.links && subtype == b"Link"
            || params.embedded_files && subtype == b"FileAttachment"
            || params.forms == SanitizeForms::Remove && subtype == b"Widget"
            || params
                .annotations_of_type
                .iter()
                .any(|s| s.as_bytes() == subtype);
        if remove {
            stats.annotations += 1;
            if subtype == b"Link" {
                stats.links += 1;
            }
            if subtype == b"FileAttachment" {
                stats.embedded += 1;
            }
            if let Ok(id) = dict.get(b"Popup").and_then(Object::as_reference) {
                removed.insert(id);
            }
        } else {
            kept.push(entry);
        }
    }
    let before_popups = kept.len();
    kept.retain(|entry| {
        entry
            .as_reference()
            .ok()
            .is_none_or(|id| !removed.contains(&id))
    });
    stats.annotations += (before_popups - kept.len()) as u64;
    doc.get_object_mut(page_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| PdfError::Malformed)?
        .set("Annots", kept);
    Ok(())
}

fn deref<'a>(
    doc: &'a Document,
    object: &'a Object,
    limits: &Limits,
) -> Result<&'a Object, PdfError> {
    let mut value = object;
    let mut seen = HashSet::new();
    while let Object::Reference(id) = value {
        if seen.len() >= limits.max_nesting_depth as usize {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: limits.max_nesting_depth,
            }));
        }
        if !seen.insert(*id) {
            return Err(PdfError::Malformed);
        }
        value = doc.objects.get(id).ok_or(PdfError::Malformed)?;
    }
    Ok(value)
}

fn prune_fields(
    doc: &mut Document,
    flattened: &[ObjectId],
    flattened_direct: &[Dictionary],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let root = catalog_object_id(doc)?;
    let Some(mut form) = doc
        .get_object(root)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"AcroForm").ok())
        .cloned()
    else {
        return Ok(());
    };
    let mut state = PruneState {
        removed: flattened.iter().copied().collect(),
        direct: flattened_direct,
        seen: HashSet::new(),
        limits,
        ctx,
        remaining: limits.max_blocks,
    };
    let empty = prune_form(doc, &mut form, &mut state, 0)?;
    let catalog = doc
        .get_object_mut(root)
        .and_then(Object::as_dict_mut)
        .map_err(|_| PdfError::Malformed)?;
    if empty {
        catalog.remove(b"AcroForm");
    } else {
        catalog.set("AcroForm", form);
    }
    Ok(())
}

struct PruneState<'a> {
    removed: HashSet<ObjectId>,
    direct: &'a [Dictionary],
    seen: HashSet<ObjectId>,
    limits: &'a Limits,
    ctx: &'a ToolCtx,
    remaining: u32,
}

fn prune_form(
    doc: &mut Document,
    obj: &mut Object,
    state: &mut PruneState<'_>,
    depth: u16,
) -> Result<bool, PdfError> {
    state.ctx.check()?;
    if state.remaining == 0 {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: state.limits.max_blocks,
        }));
    }
    state.remaining -= 1;
    if depth > state.limits.max_nesting_depth {
        return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
            limit: state.limits.max_nesting_depth,
        }));
    }
    match obj {
        Object::Reference(id) => {
            if state.removed.contains(id) {
                return Ok(true);
            }
            if !state.seen.insert(*id) {
                return Err(PdfError::Malformed);
            }
            let id = *id;
            let mut value = doc.objects.get(&id).cloned().ok_or(PdfError::Malformed)?;
            let empty = prune_form(doc, &mut value, state, depth + 1)?;
            doc.objects.insert(id, value);
            Ok(empty)
        }
        Object::Array(items) => {
            let mut kept = Vec::with_capacity(items.len());
            for mut item in std::mem::take(items) {
                if !prune_form(doc, &mut item, state, depth + 1)? {
                    kept.push(item);
                }
            }
            *items = kept;
            Ok(items.is_empty())
        }
        Object::Dictionary(dict) => {
            if state.direct.contains(dict) {
                return Ok(true);
            }
            let key = if dict.has(b"Fields") {
                b"Fields".as_slice()
            } else {
                b"Kids".as_slice()
            };
            if let Ok(kids) = dict.get_mut(key) {
                prune_form(doc, kids, state, depth + 1)
            } else {
                Ok(false)
            }
        }
        _ => Err(PdfError::Malformed),
    }
}
