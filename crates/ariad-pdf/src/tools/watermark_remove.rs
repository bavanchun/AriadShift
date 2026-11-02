//! Transactional, explicit removal of previously detected watermark occurrences.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::{WatermarkDetectParams, WatermarkRemoveParams},
};
use lopdf::{
    Dictionary, Document, Object, ObjectId, Stream,
    content::{Content, Operation},
};
use serde_json::json;

use crate::{
    PdfDoc, PdfError, ToolCtx,
    color::{page_ids, page_resources, resolve},
    document::DecodeBudget,
    draw::layer,
    naming,
    tools::{
        InputDoc, OutputDoc, ToolWarning,
        watermark_detect::source_identity,
        watermark_identity::query_from_id,
        watermark_scan::{self, Candidate},
    },
};

const MAX_SELECTED: usize = 1_000;
const MAX_QUERIES: usize = 32;

#[derive(Default)]
struct Edits {
    spans: Vec<(usize, usize, &'static str)>,
    children: BTreeMap<usize, Edits>,
}
impl Edits {
    fn add(&mut self, path: &[usize], start: usize, end: usize, kind: &'static str) {
        if let Some((head, tail)) = path.split_first() {
            self.children
                .entry(*head)
                .or_default()
                .add(tail, start, end, kind);
        } else {
            self.spans.push((start, end, kind));
        }
    }
}
fn invalid(message: &str) -> PdfError {
    PdfError::InvalidParams(message.into())
}
fn decoded(budget: &mut DecodeBudget, stream: &Stream) -> Result<Vec<u8>, PdfError> {
    if stream.dict.has(b"Filter") {
        budget.decode(stream)?.ok_or(PdfError::Malformed)
    } else {
        budget.charge(stream.content.len())?;
        Ok(stream.content.clone())
    }
}
fn resource_dict(
    doc: &Document,
    resources: &Dictionary,
    category: &[u8],
    depth: u16,
) -> Result<Dictionary, PdfError> {
    match resources.get(category) {
        Ok(value) => resolve(doc, value, depth)
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .ok_or(PdfError::Malformed),
        Err(_) => Ok(Dictionary::new()),
    }
}

#[derive(Clone, Default)]
struct Safety {
    in_text: bool,
    text_render_mode: i64,
    saved_modes: Vec<i64>,
    path_open: bool,
}
impl Safety {
    fn step(&mut self, op: &Operation) -> Result<(), PdfError> {
        match op.operator.as_str() {
            "q" => {
                if self.saved_modes.len() >= 4096 {
                    return Err(PdfError::Malformed);
                }
                self.saved_modes.push(self.text_render_mode);
            }
            "Q" => self.text_render_mode = self.saved_modes.pop().ok_or(PdfError::Malformed)?,
            "BT" => {
                if self.in_text {
                    return Err(PdfError::Malformed);
                }
                self.in_text = true;
            }
            "ET" => {
                if !self.in_text {
                    return Err(PdfError::Malformed);
                }
                self.in_text = false;
            }
            "Tr" => {
                let mode = op
                    .operands
                    .first()
                    .and_then(|v| v.as_i64().ok())
                    .filter(|v| (0..=7).contains(v))
                    .ok_or(PdfError::Malformed)?;
                self.text_render_mode = mode;
            }
            "m" | "l" | "c" | "v" | "y" | "h" | "re" => self.path_open = true,
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => self.path_open = false,
            _ => {}
        }
        Ok(())
    }
}
// A marked block must begin outside a text object with no pending path. Its own state changes,
// paths, and text objects must be fully enclosed by q/Q, with nothing left pending afterwards.
fn safe_marked(ops: &[Operation], outer_scope: bool) -> bool {
    let baseline = i32::from(outer_scope);
    let mut depth = baseline;
    let mut marked = 0_i32;
    let mut text = false;
    let mut path = false;
    for op in ops {
        match op.operator.as_str() {
            "BDC" | "BMC" => marked += 1,
            "EMC" => {
                marked -= 1;
                if marked < 0 {
                    return false;
                }
            }
            "q" => depth += 1,
            "Q" => {
                if depth == baseline + 1 && text {
                    return false;
                }
                depth -= 1;
                if depth < baseline {
                    return false;
                }
            }
            _ if depth == 0 => return false,
            "BT" => {
                if text {
                    return false;
                }
                text = true;
            }
            "ET" => {
                if !text {
                    return false;
                }
                text = false;
            }
            "m" | "l" | "c" | "v" | "y" | "h" | "re" => path = true,
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => path = false,
            "Tj" | "TJ" | "'" | "\"" if !text => return false,
            _ => {}
        }
    }
    depth == baseline && marked == 0 && !text && !path
}
fn safe_text(content: &Content, index: usize, state: &Safety) -> bool {
    if !state.in_text
        || state.text_render_mode >= 4
        || !matches!(content.operations[index].operator.as_str(), "Tj" | "TJ")
    {
        return false;
    }
    for following in content.operations.iter().skip(index + 1) {
        match following.operator.as_str() {
            "Tm" | "ET" => return true,
            "Tj" | "TJ" | "'" | "\"" | "Td" | "TD" | "T*" | "BT" => return false,
            _ => {}
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn rewrite_stream(
    doc: &mut Document,
    id: ObjectId,
    resources: &mut Dictionary,
    edits: &Edits,
    budget: &mut DecodeBudget,
    limits: &Limits,
    ctx: &ToolCtx,
    depth: u16,
    initial: &Safety,
    removed_properties: &mut HashSet<Vec<u8>>,
) -> Result<ObjectId, PdfError> {
    ctx.check()?;
    if depth > limits.max_nesting_depth.min(64) {
        return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
            limit: limits.max_nesting_depth.min(64),
        }));
    }
    let source = doc
        .get_object(id)
        .and_then(Object::as_stream)
        .map_err(|_| PdfError::Malformed)?;
    let size = crate::color::targeted_document::dictionary_size(
        &source.dict,
        0,
        limits.max_nesting_depth,
    )?;
    budget.charge(source.content.len().saturating_add(size.saturating_mul(8)))?;
    let mut stream = source.clone();
    let data = decoded(budget, &stream)?;
    let mut content = Content::decode(&data).map_err(|_| PdfError::Malformed)?;
    let mut spans = edits.spans.clone();
    spans.sort_unstable_by_key(|s| s.0);
    let mut last = 0;
    for &(start, end, _) in &spans {
        if start >= end
            || end > content.operations.len()
            || start < last
            || edits.children.keys().any(|i| *i >= start && *i < end)
        {
            return Err(invalid(
                "selected watermark occurrences overlap or are ambiguous",
            ));
        }
        last = end;
    }
    let starts: BTreeMap<usize, (usize, &'static str)> = spans
        .iter()
        .map(|&(start, end, kind)| (start, (end, kind)))
        .collect();
    let mut state = initial.clone();
    for index in 0..content.operations.len() {
        if index % 1024 == 0 {
            ctx.check()?;
        }
        if let Some(&(end, kind)) = starts.get(&index) {
            if kind == "ocg"
                && depth == 0
                && let Some(name) = content.operations[index]
                    .operands
                    .get(1)
                    .and_then(|v| v.as_name().ok())
            {
                removed_properties.insert(name.to_vec());
            }
            match kind {
                "ocg" | "artifact"
                    if state.in_text
                        || state.path_open
                        || !safe_marked(
                            &content.operations[index..end],
                            index > 0
                                && content.operations[index - 1].operator == "q"
                                && content
                                    .operations
                                    .get(end)
                                    .is_some_and(|op| op.operator == "Q"),
                        ) =>
                {
                    return Err(invalid(
                        "watermark block has state changes that cannot be removed safely",
                    ));
                }
                "repeated_text" if end != index + 1 || !safe_text(&content, index, &state) => {
                    return Err(invalid(
                        "watermark text shares a text state that cannot be removed safely",
                    ));
                }
                "repeated_xobject"
                    if end != index + 1 || content.operations[index].operator != "Do" =>
                {
                    return Err(invalid("watermark object occurrence is ambiguous"));
                }
                _ => {}
            }
        }
        if let Some(child) = edits.children.get(&index) {
            if state.in_text {
                return Err(invalid("watermark form occurs inside a text object"));
            }
            let op = &content.operations[index];
            if op.operator != "Do" {
                return Err(invalid("watermark form occurrence is ambiguous"));
            }
            let name = op
                .operands
                .first()
                .and_then(|o| o.as_name().ok())
                .ok_or(PdfError::Malformed)?;
            let entries = resource_dict(doc, resources, b"XObject", limits.max_nesting_depth)?;
            let child_id = entries
                .get(name)
                .and_then(Object::as_reference)
                .map_err(|_| PdfError::Malformed)?;
            let form = doc
                .get_object(child_id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?;
            if form.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
                return Err(invalid("watermark path does not name a form"));
            }
            let mut form_resources = match form.dict.get(b"Resources") {
                Ok(value) => resolve(doc, value, limits.max_nesting_depth)
                    .and_then(|o| o.as_dict().ok())
                    .cloned()
                    .ok_or(PdfError::Malformed)?,
                Err(_) => resources.clone(),
            };
            let form_state = Safety {
                text_render_mode: state.text_render_mode,
                ..Safety::default()
            };
            let new_id = rewrite_stream(
                doc,
                child_id,
                &mut form_resources,
                child,
                budget,
                limits,
                ctx,
                depth.saturating_add(1),
                &form_state,
                removed_properties,
            )?;
            doc.get_object_mut(new_id)
                .and_then(Object::as_stream_mut)
                .map_err(|_| PdfError::Malformed)?
                .dict
                .set("Resources", form_resources);
            let alias = layer::insert_resource(
                doc,
                resources,
                b"XObject",
                "wm",
                Object::Reference(new_id),
            )?;
            content.operations[index].operands[0] = Object::Name(alias);
        }
        state.step(&content.operations[index])?;
    }
    for (start, end, _) in spans.into_iter().rev() {
        content.operations.drain(start..end);
    }
    let encoded = content.encode().map_err(|_| PdfError::Malformed)?;
    budget.charge(encoded.len())?;
    stream.set_plain_content(encoded);
    Ok(doc.add_object(stream))
}

pub(super) fn contents(
    doc: &Document,
    page: ObjectId,
    depth: u16,
) -> Result<Vec<ObjectId>, PdfError> {
    fn follow<'a>(
        doc: &'a Document,
        value: &'a Object,
        limit: u16,
    ) -> Result<(Option<ObjectId>, &'a Object), PdfError> {
        let mut current = value;
        let mut last = None;
        let mut seen = HashSet::new();
        while let Object::Reference(id) = current {
            if seen.len() >= usize::from(limit) || !seen.insert(*id) {
                return Err(PdfError::Malformed);
            }
            last = Some(*id);
            current = doc.objects.get(id).ok_or(PdfError::Malformed)?;
        }
        Ok((last, current))
    }
    let page = doc.get_dictionary(page).map_err(|_| PdfError::Malformed)?;
    let Ok(value) = page.get(b"Contents") else {
        return Ok(Vec::new());
    };
    let (id, resolved) = follow(doc, value, depth)?;
    match resolved {
        Object::Stream(_) => Ok(vec![id.ok_or(PdfError::Malformed)?]),
        Object::Array(items) => {
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                let (id, value) = follow(doc, item, depth)?;
                match value {
                    Object::Null => {}
                    Object::Stream(_) => result.push(id.ok_or(PdfError::Malformed)?),
                    _ => return Err(PdfError::Malformed),
                }
            }
            Ok(result)
        }
        Object::Null => Ok(Vec::new()),
        _ => Err(PdfError::Malformed),
    }
}
fn marker(doc: &Document, id: ObjectId) -> Option<&[u8]> {
    doc.get_object(id)
        .ok()?
        .as_stream()
        .ok()?
        .dict
        .get(b"AriadShift")
        .ok()?
        .as_name()
        .ok()
}
fn remove_own_streams(
    doc: &Document,
    stream_ids: &mut Vec<ObjectId>,
    removed: &BTreeMap<usize, usize>,
    budget: &mut DecodeBudget,
) -> Result<HashSet<Vec<u8>>, PdfError> {
    let mut names = HashSet::new();
    for (slot, expected_end) in removed {
        let id = *stream_ids.get(*slot).ok_or(PdfError::Malformed)?;
        if marker(doc, id) != Some(layer::MARKER_WATERMARK) {
            return Err(invalid("watermark marker stream is ambiguous"));
        }
        let stream = doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let content =
            Content::decode(&decoded(budget, stream)?).map_err(|_| PdfError::Malformed)?;
        let ops = &content.operations;
        if ops.len() < 6
            || *expected_end != ops.len() - 1
            || ops[0].operator != "q"
            || ops[1].operator != "BDC"
            || ops[2].operator != "BDC"
            || ops[ops.len() - 3].operator != "EMC"
            || ops[ops.len() - 2].operator != "EMC"
            || ops[ops.len() - 1].operator != "Q"
            || !safe_marked(ops, false)
        {
            return Err(invalid(
                "watermark marker stream does not have an isolated structure",
            ));
        }
        if !collect_names(ops, &mut names)? {
            return Err(invalid("watermark marker stream has ambiguous resources"));
        }
    }
    for slot in removed.keys().rev() {
        stream_ids.remove(*slot);
    }
    let begins: Vec<usize> = stream_ids
        .iter()
        .enumerate()
        .filter_map(|(i, id)| (marker(doc, *id) == Some(layer::MARKER_WRAP_BEGIN)).then_some(i))
        .collect();
    let ends: Vec<usize> = stream_ids
        .iter()
        .enumerate()
        .filter_map(|(i, id)| (marker(doc, *id) == Some(layer::MARKER_WRAP_END)).then_some(i))
        .collect();
    if begins.len() == 1
        && ends.len() == 1
        && begins[0] < ends[0]
        && ends[0] + 1 == stream_ids.len()
    {
        let begin = doc
            .get_object(stream_ids[begins[0]])
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let end = doc
            .get_object(stream_ids[ends[0]])
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let first = Content::decode(&decoded(budget, begin)?).map_err(|_| PdfError::Malformed)?;
        let last = Content::decode(&decoded(budget, end)?).map_err(|_| PdfError::Malformed)?;
        if first.operations.len() == 1
            && first.operations[0].operator == "q"
            && last.operations.len() == 1
            && last.operations[0].operator == "Q"
        {
            stream_ids.remove(ends[0]);
            stream_ids.remove(begins[0]);
        }
    }
    Ok(names)
}

/// Removes only selected detector-issued identifiers and emits an inline report.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &WatermarkRemoveParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    if inputs.len() != 1 {
        return Err(invalid("pdf.watermark_remove requires exactly one input"));
    }
    if params.candidates.len() > MAX_SELECTED {
        return Err(invalid("too many watermark candidates selected"));
    }
    let original = inputs[0].open_for_mutation(ctx)?;
    let (input_hash, input_len) = source_identity(&inputs[0])?;
    if params.validate().is_err() {
        return Err(invalid_selection(
            &original,
            &input_hash,
            input_len,
            &inputs[0].limits,
            ctx,
        )?);
    }
    let mut queries: BTreeMap<String, WatermarkDetectParams> = BTreeMap::new();
    for id in &params.candidates {
        let Some(query) = query_from_id(id) else {
            return Err(invalid_selection(
                &original,
                &input_hash,
                input_len,
                &inputs[0].limits,
                ctx,
            )?);
        };
        let key = serde_json::to_string(&query).map_err(|_| PdfError::Malformed)?;
        queries.insert(key, query);
    }
    if queries.len() > MAX_QUERIES {
        return Err(invalid("too many distinct watermark detection queries"));
    }
    let mut found: BTreeMap<String, Candidate> = BTreeMap::new();
    for query in queries.values() {
        ctx.check()?;
        let detection = watermark_scan::scan(
            &original,
            &input_hash,
            input_len,
            query,
            &inputs[0].limits,
            ctx,
        )?;
        for candidate in detection.candidates {
            found.insert(candidate.id.clone(), candidate);
        }
    }
    let mut selected = Vec::new();
    for id in &params.candidates {
        if let Some(candidate) = found.get(id) {
            selected.push(candidate.clone());
        } else {
            let valid = found
                .keys()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            return Err(PdfError::InvalidParams(format!(
                "unknown watermark candidate; valid ids: {valid}"
            )));
        }
    }
    let selected_ids: BTreeSet<&str> = params.candidates.iter().map(String::as_str).collect();
    let not_selected: Vec<String> = found
        .keys()
        .filter(|id| !selected_ids.contains(id.as_str()))
        .cloned()
        .collect();
    let mut trees: BTreeMap<(u32, usize), Edits> = BTreeMap::new();
    let mut own: BTreeMap<u32, BTreeMap<usize, usize>> = BTreeMap::new();
    let mut annotations: BTreeMap<u32, BTreeMap<usize, Option<ObjectId>>> = BTreeMap::new();
    for candidate in &selected {
        for occurrence in &candidate.occurrences {
            if occurrence.ambiguous {
                return Err(invalid(
                    "watermark spans multiple content streams and cannot be removed safely",
                ));
            }
            if candidate.kind == "annotation" {
                if annotations
                    .entry(occurrence.page)
                    .or_default()
                    .insert(occurrence.start, occurrence.annotation)
                    .is_some()
                {
                    return Err(invalid("selected watermark occurrences overlap"));
                }
            } else if occurrence.own_stream {
                if occurrence.start != 1
                    || own
                        .entry(occurrence.page)
                        .or_default()
                        .insert(occurrence.slot, occurrence.end)
                        .is_some()
                {
                    return Err(invalid(
                        "selected watermark occurrences overlap or the marker stream is ambiguous",
                    ));
                }
            } else {
                trees
                    .entry((occurrence.page, occurrence.slot))
                    .or_default()
                    .add(
                        &occurrence.path,
                        occurrence.start,
                        occurrence.end,
                        candidate.kind,
                    );
            }
        }
    }
    for (page, slots) in &own {
        if slots.keys().any(|slot| trees.contains_key(&(*page, *slot))) {
            return Err(invalid("selected watermark occurrences overlap"));
        }
    }
    let selected_groups: BTreeSet<ObjectId> = selected
        .iter()
        .filter_map(|candidate| candidate.ocg)
        .collect();
    let mut output = original.clone();
    let mut budget = output.decode_budget();
    let doc = output.inner_mut()?;
    let page_ids = page_ids(doc, &inputs[0].limits, ctx)?;
    let mut affected: BTreeSet<u32> = trees.keys().map(|(p, _)| *p).collect();
    affected.extend(own.keys().copied());
    affected.extend(annotations.keys().copied());
    for page_number in affected {
        ctx.check()?;
        let page = *page_ids
            .get((page_number - 1) as usize)
            .ok_or(PdfError::Malformed)?;
        let mut resources = page_resources(doc, page, &inputs[0].limits)?;
        let mut streams = contents(doc, page, inputs[0].limits.max_nesting_depth)?;
        let mut state = Safety::default();
        let mut removed_properties = HashSet::new();
        for (slot, stream_id) in streams.iter_mut().enumerate() {
            let original_id = *stream_id;
            if own
                .get(&page_number)
                .is_some_and(|slots| slots.contains_key(&slot))
                && (state.in_text || state.path_open)
            {
                return Err(invalid(
                    "watermark marker stream is not isolated from preceding content",
                ));
            }
            let original_stream = doc
                .get_object(original_id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?;
            let original_content = Content::decode(&decoded(&mut budget, original_stream)?)
                .map_err(|_| PdfError::Malformed)?;
            if let Some(tree) = trees.get(&(page_number, slot)) {
                *stream_id = rewrite_stream(
                    doc,
                    original_id,
                    &mut resources,
                    tree,
                    &mut budget,
                    &inputs[0].limits,
                    ctx,
                    0,
                    &state,
                    &mut removed_properties,
                )?;
            }
            for op in &original_content.operations {
                state.step(op)?;
            }
        }
        let removed_names = if let Some(slots) = own.get(&page_number) {
            remove_own_streams(doc, &mut streams, slots, &mut budget)?
        } else {
            HashSet::new()
        };
        let mut annots = if let Some(selected) = annotations.get(&page_number) {
            let object = doc
                .get_dictionary(page)
                .map_err(|_| PdfError::Malformed)?
                .get(b"Annots")
                .map_err(|_| PdfError::Malformed)?;
            let mut values = resolve(doc, object, inputs[0].limits.max_nesting_depth)
                .and_then(|o| o.as_array().ok())
                .cloned()
                .ok_or(PdfError::Malformed)?;
            for (index, expected) in selected.iter().rev() {
                if *index >= values.len() || values[*index].as_reference().ok() != *expected {
                    return Err(PdfError::Malformed);
                }
                values.remove(*index);
            }
            Some(values)
        } else {
            None
        };
        if let Some(values) = annots.take() {
            let page_dict = doc
                .get_dictionary_mut(page)
                .map_err(|_| PdfError::Malformed)?;
            if values.is_empty() {
                page_dict.remove(b"Annots");
            } else {
                page_dict.set("Annots", Object::Array(values));
            }
        }
        if !removed_names.is_empty() || !removed_properties.is_empty() {
            remove_unused_resources(
                doc,
                page,
                &mut resources,
                &streams,
                &removed_names,
                &removed_properties,
                &mut budget,
                &inputs[0].limits,
                ctx,
            )?;
        }
        let page_dict = doc
            .get_dictionary_mut(page)
            .map_err(|_| PdfError::Malformed)?;
        page_dict.set("Resources", resources);
        page_dict.set(
            "Contents",
            Object::Array(streams.into_iter().map(Object::Reference).collect()),
        );
    }
    for id in selected_groups {
        prune_own_ocg(doc, id, &page_ids, &mut budget, &inputs[0].limits, ctx)?;
    }
    let report:Vec<_>=selected.iter().map(|candidate| {
        let mut per_page:BTreeMap<u32,(usize,usize)>=BTreeMap::new();
        for occurrence in &candidate.occurrences { let entry=per_page.entry(occurrence.page).or_default(); if candidate.kind=="annotation" || occurrence.own_stream {entry.0+=1;} else {entry.1+=occurrence.end-occurrence.start;} }
        json!({"id":candidate.id,"kind":candidate.kind,"pages":per_page.into_iter().map(|(page,(objects_removed,ops_removed))|json!({"page":page,"objects_removed":objects_removed,"ops_removed":ops_removed})).collect::<Vec<_>>()})
    }).collect();
    let mut result = OutputDoc::from_pdf(
        naming::default_pdf_output_name(&inputs[0].clean_stem(), "pdf.watermark_remove"),
        output,
    );
    result.json = Some(json!({"removed":report,"not_selected":not_selected}));
    if selected.iter().any(|candidate| candidate.origin == "other") {
        result.coded_warnings.push(ToolWarning {code:"third_party_mark".into(),message:"removed a mark not added by AriadShift; use this only on documents you own or may modify".into()});
    }
    Ok((vec![original], vec![result]))
}

// Names in content operands are the only resource identifiers a content stream may reference.
// If an indirect operand prevents a complete scan, retain all potentially shared resources.
fn collect_names(ops: &[Operation], names: &mut HashSet<Vec<u8>>) -> Result<bool, PdfError> {
    let mut visited = 0_usize;
    for op in ops {
        let mut stack: Vec<(&Object, usize)> = op.operands.iter().map(|v| (v, 0)).collect();
        while let Some((value, depth)) = stack.pop() {
            visited += 1;
            if visited > 4_000_000 || depth > 64 {
                return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                    limit: 4_000_000,
                }));
            }
            match value {
                Object::Name(name) => {
                    names.insert(name.clone());
                    if names.len() > 100_000 {
                        return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                            limit: 100_000,
                        }));
                    }
                }
                Object::Array(items) => stack.extend(items.iter().map(|v| (v, depth + 1))),
                Object::Dictionary(items) => {
                    stack.extend(items.iter().map(|(_, v)| (v, depth + 1)))
                }
                Object::Reference(_) | Object::Stream(_) => return Ok(false),
                _ => {}
            }
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn collect_resource_uses(
    doc: &Document,
    resources: &Dictionary,
    streams: &[ObjectId],
    names: &mut HashSet<Vec<u8>>,
    budget: &mut DecodeBudget,
    limits: &Limits,
    ctx: &ToolCtx,
    active: &mut HashSet<ObjectId>,
    visits: &mut usize,
    depth: u16,
) -> Result<bool, PdfError> {
    if depth > limits.max_nesting_depth.min(64) || !simple_resources(doc, resources, limits)? {
        return Ok(false);
    }
    for id in streams {
        ctx.check()?;
        if !active.insert(*id) {
            return Ok(false);
        }
        *visits += 1;
        if *visits > 100_000 {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: 100_000,
            }));
        }
        let stream = doc
            .get_object(*id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let data = decoded(budget, stream)?;
        let content = Content::decode(&data).map_err(|_| PdfError::Malformed)?;
        if !collect_names(&content.operations, names)? {
            return Ok(false);
        }
        for op in &content.operations {
            if op.operator != "Do" {
                continue;
            }
            let Some(name) = op.operands.first().and_then(|v| v.as_name().ok()) else {
                return Ok(false);
            };
            let entries = resource_dict(doc, resources, b"XObject", limits.max_nesting_depth)?;
            let Some(child) = entries.get(name).ok().and_then(|v| v.as_reference().ok()) else {
                return Ok(false);
            };
            let object = doc
                .get_object(child)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?;
            let subtype = object.dict.get(b"Subtype").and_then(Object::as_name).ok();
            if subtype == Some(b"Form") {
                if object.dict.has(b"Group") || object.dict.has(b"OC") {
                    return Ok(false);
                }
                let nested = match object.dict.get(b"Resources") {
                    Ok(value) => resolve(doc, value, limits.max_nesting_depth)
                        .and_then(|v| v.as_dict().ok())
                        .cloned()
                        .ok_or(PdfError::Malformed)?,
                    Err(_) => resources.clone(),
                };
                if !collect_resource_uses(
                    doc,
                    &nested,
                    &[child],
                    names,
                    budget,
                    limits,
                    ctx,
                    active,
                    visits,
                    depth.saturating_add(1),
                )? {
                    return Ok(false);
                }
            } else if subtype == Some(b"Image") {
                for key in [b"SMask".as_slice(), b"Mask"] {
                    if let Ok(mask) = object.dict.get(key) {
                        let Some(mask) = resolve(doc, mask, limits.max_nesting_depth) else {
                            return Ok(false);
                        };
                        if let Object::Stream(mask) = mask {
                            if mask.dict.has(b"SMask") || mask.dict.has(b"Mask") {
                                return Ok(false);
                            }
                            if let Ok(space) = mask.dict.get(b"ColorSpace") {
                                let synthetic = Operation::new("", vec![space.clone()]);
                                if !collect_names(&[synthetic], names)? {
                                    return Ok(false);
                                }
                            }
                        } else if !matches!(mask, Object::Array(_) | Object::Name(_)) {
                            return Ok(false);
                        }
                    }
                }
                if let Ok(space) = object.dict.get(b"ColorSpace") {
                    let synthetic = Operation::new("", vec![space.clone()]);
                    if !collect_names(&[synthetic], names)? {
                        return Ok(false);
                    }
                }
            } else {
                return Ok(false);
            }
        }
        active.remove(id);
    }
    Ok(true)
}
#[allow(clippy::too_many_arguments)]
fn annotation_resource_uses(
    doc: &Document,
    page: ObjectId,
    resources: &Dictionary,
    names: &mut HashSet<Vec<u8>>,
    budget: &mut DecodeBudget,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    let page = doc.get_dictionary(page).map_err(|_| PdfError::Malformed)?;
    let Some(annotations) = page.get(b"Annots").ok() else {
        return Ok(true);
    };
    let Some(annotations) =
        resolve(doc, annotations, limits.max_nesting_depth).and_then(|v| v.as_array().ok())
    else {
        return Ok(false);
    };
    let mut roots = Vec::new();
    for annotation in annotations {
        let Some(annotation) =
            resolve(doc, annotation, limits.max_nesting_depth).and_then(|v| v.as_dict().ok())
        else {
            return Ok(false);
        };
        if let Ok(appearance) = annotation.get(b"AP") {
            roots.push((appearance, 0_u16));
        }
    }
    let mut seen = HashSet::new();
    let mut streams = Vec::new();
    while let Some((value, depth)) = roots.pop() {
        ctx.check()?;
        if depth > limits.max_nesting_depth.min(64) {
            return Ok(false);
        }
        match value {
            Object::Reference(id) => {
                if !seen.insert(*id) {
                    continue;
                }
                if seen.len() > 100_000 {
                    return Ok(false);
                }
                let Ok(value) = doc.get_object(*id) else {
                    return Ok(false);
                };
                if matches!(value, Object::Stream(_)) {
                    streams.push(*id);
                } else {
                    roots.push((value, depth + 1));
                }
            }
            Object::Dictionary(dict) => {
                budget.charge(crate::color::targeted_document::dictionary_size(
                    dict,
                    0,
                    limits.max_nesting_depth,
                )?)?;
                roots.extend(dict.iter().map(|(_, value)| (value, depth + 1)));
            }
            Object::Name(_) | Object::Null => {}
            _ => return Ok(false),
        }
    }
    for id in streams {
        let stream = doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let appearance_resources = match stream.dict.get(b"Resources") {
            Ok(value) => {
                match resolve(doc, value, limits.max_nesting_depth).and_then(|v| v.as_dict().ok()) {
                    Some(dict) => dict,
                    None => return Ok(false),
                }
            }
            Err(_) => resources,
        };
        if !collect_resource_uses(
            doc,
            appearance_resources,
            &[id],
            names,
            budget,
            limits,
            ctx,
            &mut HashSet::new(),
            &mut 0,
            0,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn remove_unused_resources(
    doc: &Document,
    page: ObjectId,
    resources: &mut Dictionary,
    streams: &[ObjectId],
    removed_names: &HashSet<Vec<u8>>,
    removed_properties: &HashSet<Vec<u8>>,
    budget: &mut DecodeBudget,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if !simple_resources(doc, resources, limits)? {
        return Ok(());
    }
    let mut used = HashSet::new();
    if !annotation_resource_uses(doc, page, resources, &mut used, budget, limits, ctx)? {
        return Ok(());
    }
    if !soft_mask_resource_uses(doc, resources, &mut used, budget, limits, ctx)? {
        return Ok(());
    }
    if !collect_resource_uses(
        doc,
        resources,
        streams,
        &mut used,
        budget,
        limits,
        ctx,
        &mut HashSet::new(),
        &mut 0,
        0,
    )? {
        return Ok(());
    }
    for category in [
        b"Properties".as_slice(),
        b"XObject",
        b"Font",
        b"ExtGState",
        b"ColorSpace",
        b"Pattern",
        b"Shading",
    ] {
        if !resources.has(category) {
            continue;
        }
        let mut entries = resource_dict(doc, resources, category, limits.max_nesting_depth)?;
        let keys: Vec<Vec<u8>> = entries
            .iter()
            .filter(|(key, _)| {
                ((key.starts_with(b"AS") && removed_names.contains(*key))
                    || (category == b"Properties" && removed_properties.contains(*key)))
                    && !used.contains(*key)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            entries.remove(&key);
        }
        resources.set(category.to_vec(), entries);
    }
    Ok(())
}

fn simple_resources(
    doc: &Document,
    resources: &Dictionary,
    limits: &Limits,
) -> Result<bool, PdfError> {
    if resources.has(b"Pattern") || resources.has(b"Shading") {
        return Ok(false);
    }
    for (_, font) in resource_dict(doc, resources, b"Font", limits.max_nesting_depth)?.iter() {
        let Some(font) =
            resolve(doc, font, limits.max_nesting_depth).and_then(|v| v.as_dict().ok())
        else {
            return Ok(false);
        };
        match font
            .get(b"Subtype")
            .ok()
            .and_then(|value| resolve(doc, value, limits.max_nesting_depth))
            .and_then(|value| value.as_name().ok())
        {
            Some(b"Type3") | None => return Ok(false),
            Some(_) => {}
        }
    }
    Ok(true)
}

fn soft_mask_resource_uses(
    doc: &Document,
    resources: &Dictionary,
    names: &mut HashSet<Vec<u8>>,
    budget: &mut DecodeBudget,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    for (_, gs) in resource_dict(doc, resources, b"ExtGState", limits.max_nesting_depth)?.iter() {
        let Some(gs) = resolve(doc, gs, limits.max_nesting_depth).and_then(|v| v.as_dict().ok())
        else {
            return Ok(false);
        };
        let Ok(mask) = gs.get(b"SMask") else {
            continue;
        };
        let Some(mask) = resolve(doc, mask, limits.max_nesting_depth) else {
            return Ok(false);
        };
        if mask.as_name().ok() == Some(b"None") {
            continue;
        }
        let Some(group) = mask
            .as_dict()
            .ok()
            .and_then(|dict| dict.get(b"G").ok())
            .and_then(|v| v.as_reference().ok())
        else {
            return Ok(false);
        };
        let stream = doc
            .get_object(group)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let group_resources = match stream.dict.get(b"Resources") {
            Ok(value) => {
                match resolve(doc, value, limits.max_nesting_depth).and_then(|v| v.as_dict().ok()) {
                    Some(dict) => dict,
                    None => return Ok(false),
                }
            }
            Err(_) => resources,
        };
        if !collect_resource_uses(
            doc,
            group_resources,
            &[group],
            names,
            budget,
            limits,
            ctx,
            &mut HashSet::new(),
            &mut 0,
            0,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn filter_group_array(
    doc: &mut Document,
    object: &Object,
    target: ObjectId,
    depth: u16,
    limit: u16,
    count: &mut usize,
    ctx: &ToolCtx,
    budget: &mut DecodeBudget,
    memo: &mut BTreeMap<ObjectId, ObjectId>,
    active: &mut HashSet<ObjectId>,
) -> Result<Object, PdfError> {
    if depth > limit.min(64) {
        return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
            limit: limit.min(64),
        }));
    }
    let reference = object.as_reference().ok();
    if let Some(id) = reference {
        if active.contains(&id) {
            return Err(PdfError::Malformed);
        }
        if let Some(new_id) = memo.get(&id) {
            return Ok(Object::Reference(*new_id));
        }
        active.insert(id);
    }
    let values = resolve(doc, object, limit)
        .and_then(|v| v.as_array().ok())
        .ok_or(PdfError::Malformed)?;
    if values.len() > 100_000 {
        return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
            limit: 100_000,
        }));
    }
    budget.charge(values.len().saturating_mul(std::mem::size_of::<Object>()))?;
    for item in values {
        budget.charge(crate::color::targeted_document::resource_size(
            item, 0, limit,
        )?)?;
    }
    let values = values.clone();
    let mut out = Vec::with_capacity(values.len());
    for item in &values {
        *count = count.saturating_add(1);
        if *count > 4_000_000 {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: 4_000_000,
            }));
        }
        if (*count).is_multiple_of(1024) {
            ctx.check()?;
        }
        if item.as_reference().ok() == Some(target) {
            continue;
        }
        if resolve(doc, item, limit).is_some_and(|v| matches!(v, Object::Array(_))) {
            out.push(filter_group_array(
                doc,
                item,
                target,
                depth.saturating_add(1),
                limit,
                count,
                ctx,
                budget,
                memo,
                active,
            )?);
        } else {
            out.push(item.clone());
        }
    }
    if let Some(id) = reference {
        active.remove(&id);
        let new_id = doc.add_object(Object::Array(out));
        memo.insert(id, new_id);
        Ok(Object::Reference(new_id))
    } else {
        Ok(Object::Array(out))
    }
}
fn properties_without_group(
    doc: &mut Document,
    object: &Object,
    target: ObjectId,
    limit: u16,
    ctx: &ToolCtx,
    budget: &mut DecodeBudget,
) -> Result<Dictionary, PdfError> {
    let mut count = 0;
    let mut memo = BTreeMap::new();
    let mut active = HashSet::new();
    let props_ref = resolve(doc, object, limit)
        .and_then(|v| v.as_dict().ok())
        .ok_or(PdfError::Malformed)?;
    budget.charge(
        crate::color::targeted_document::dictionary_size(props_ref, 0, limit)?.saturating_mul(2),
    )?;
    let mut props = props_ref.clone();
    if let Ok(groups) = props.get(b"OCGs").cloned() {
        props.set(
            "OCGs",
            filter_group_array(
                doc,
                &groups,
                target,
                0,
                limit,
                &mut count,
                ctx,
                budget,
                &mut memo,
                &mut active,
            )?,
        );
    }
    if let Ok(defaults) = props.get(b"D") {
        let mut defaults = resolve(doc, defaults, limit)
            .and_then(|v| v.as_dict().ok())
            .cloned()
            .ok_or(PdfError::Malformed)?;
        for key in [b"Order".as_slice(), b"ON", b"OFF", b"Locked", b"RBGroups"] {
            if let Ok(value) = defaults.get(key).cloned() {
                defaults.set(
                    key.to_vec(),
                    filter_group_array(
                        doc,
                        &value,
                        target,
                        0,
                        limit,
                        &mut count,
                        ctx,
                        budget,
                        &mut memo,
                        &mut active,
                    )?,
                );
            }
        }
        props.set("D", defaults);
    }
    Ok(props)
}
fn references_group(
    doc: &Document,
    catalog: ObjectId,
    props: &Dictionary,
    target: ObjectId,
    ctx: &ToolCtx,
    limit: u16,
) -> Result<bool, PdfError> {
    use std::collections::VecDeque;
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    let mut count = 0_usize;
    fn visit(
        value: &Object,
        target: ObjectId,
        seen: &mut HashSet<ObjectId>,
        queue: &mut VecDeque<ObjectId>,
        count: &mut usize,
        limit: u16,
        ctx: &ToolCtx,
    ) -> Result<bool, PdfError> {
        let mut stack = vec![(value, 0_u16)];
        while let Some((value, depth)) = stack.pop() {
            *count = count.saturating_add(1);
            if (*count).is_multiple_of(1024) {
                ctx.check()?;
            }
            if *count > 4_000_000 {
                return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                    limit: 4_000_000,
                }));
            }
            if depth > limit.min(64) {
                return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                    limit: limit.min(64),
                }));
            }
            match value {
                Object::Reference(id) if *id == target => return Ok(true),
                Object::Reference(id) => {
                    if seen.insert(*id) {
                        queue.push_back(*id);
                    }
                }
                Object::Array(items) => {
                    stack.extend(items.iter().map(|v| (v, depth.saturating_add(1))))
                }
                Object::Dictionary(items) => {
                    stack.extend(items.iter().map(|(_, v)| (v, depth.saturating_add(1))))
                }
                Object::Stream(stream) => stack.extend(
                    stream
                        .dict
                        .iter()
                        .map(|(_, v)| (v, depth.saturating_add(1))),
                ),
                _ => {}
            }
        }
        Ok(false)
    }
    for (_, value) in doc.trailer.iter() {
        if visit(value, target, &mut seen, &mut queue, &mut count, limit, ctx)? {
            return Ok(true);
        }
    }
    while let Some(id) = queue.pop_front() {
        if count.is_multiple_of(1024) {
            ctx.check()?;
        }
        let Some(object) = doc.objects.get(&id) else {
            continue;
        };
        if id == catalog {
            let dict = object.as_dict().map_err(|_| PdfError::Malformed)?;
            for (key, value) in dict.iter() {
                if key != b"OCProperties"
                    && visit(value, target, &mut seen, &mut queue, &mut count, limit, ctx)?
                {
                    return Ok(true);
                }
            }
            let replacement = Object::Dictionary(props.clone());
            if visit(
                &replacement,
                target,
                &mut seen,
                &mut queue,
                &mut count,
                limit,
                ctx,
            )? {
                return Ok(true);
            }
        } else if visit(
            object, target, &mut seen, &mut queue, &mut count, limit, ctx,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn prune_own_ocg(
    doc: &mut Document,
    target: ObjectId,
    pages: &[ObjectId],
    budget: &mut DecodeBudget,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    // Refuse to infer reachability when content names or nested content cannot be exhaustively
    // inspected. In that case the harmless group is retained rather than risking unrelated marks.
    let mut active = HashSet::new();
    let mut visits = 0;
    for page in pages {
        ctx.check()?;
        let resources = page_resources(doc, *page, limits)?;
        if !annotation_resource_uses(
            doc,
            *page,
            &resources,
            &mut HashSet::new(),
            budget,
            limits,
            ctx,
        )? {
            return Ok(());
        }
        if !soft_mask_resource_uses(doc, &resources, &mut HashSet::new(), budget, limits, ctx)? {
            return Ok(());
        }
        let streams = contents(doc, *page, limits.max_nesting_depth)?;
        if !collect_resource_uses(
            doc,
            &resources,
            &streams,
            &mut HashSet::new(),
            budget,
            limits,
            ctx,
            &mut active,
            &mut visits,
            0,
        )? {
            return Ok(());
        }
    }
    let catalog = crate::tools::metadata::catalog_object_id(doc)?;
    let Some(object) = doc
        .get_dictionary(catalog)
        .map_err(|_| PdfError::Malformed)?
        .get(b"OCProperties")
        .ok()
    else {
        return Ok(());
    };
    budget.charge(crate::color::targeted_document::resource_size(
        object,
        0,
        limits.max_nesting_depth,
    )?)?;
    let object = object.clone();
    let updated =
        match properties_without_group(doc, &object, target, limits.max_nesting_depth, ctx, budget)
        {
            Ok(updated) => updated,
            Err(PdfError::Malformed) => return Ok(()),
            Err(error) => return Err(error),
        };
    budget.charge(crate::color::targeted_document::dictionary_size(
        &updated,
        0,
        limits.max_nesting_depth,
    )?)?;
    if !references_group(
        doc,
        catalog,
        &updated,
        target,
        ctx,
        limits.max_nesting_depth,
    )? {
        let empty = updated
            .get(b"OCGs")
            .ok()
            .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
            .and_then(|v| v.as_array().ok())
            .is_some_and(Vec::is_empty)
            && !updated.has(b"Configs");
        let catalog_dict = doc
            .get_dictionary_mut(catalog)
            .map_err(|_| PdfError::Malformed)?;
        if empty {
            catalog_dict.remove(b"OCProperties");
        } else {
            catalog_dict.set("OCProperties", updated);
        }
    }
    Ok(())
}

fn invalid_selection(
    pdf: &PdfDoc,
    hash: &[u8; 32],
    input_len: usize,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<PdfError, PdfError> {
    let detection = watermark_scan::scan(
        pdf,
        hash,
        input_len,
        &WatermarkDetectParams::default(),
        limits,
        ctx,
    )?;
    let valid = detection
        .candidates
        .iter()
        .take(20)
        .map(|candidate| candidate.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(PdfError::InvalidParams(format!(
        "watermark candidates must be unique explicit identifiers; valid ids: {valid}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn shared_optional_content_arrays_do_not_expand() {
        let mut doc = Document::with_version("1.5");
        let target = doc.add_object(dictionary! {"Type" => "OCG", "Name" => "Draft"});
        let mut id = doc.add_object(Object::Array(vec![Object::Reference(target)]));
        for _ in 0..21 {
            id = doc.add_object(Object::Array(vec![
                Object::Reference(id),
                Object::Reference(id),
            ]));
        }
        let props = Object::Dictionary(
            dictionary! {"OCGs" => vec![Object::Reference(target)], "D" => dictionary! {"Order" => id, "ON" => id}},
        );
        let before = doc.objects.len();
        let mut budget = DecodeBudget::from_limits(4096, &Limits::local());
        let updated =
            properties_without_group(&mut doc, &props, target, 64, &ToolCtx::new(), &mut budget)
                .unwrap();
        assert!(doc.objects.len() - before <= 22);
        let order = updated
            .get(b"D")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Order")
            .unwrap()
            .as_reference()
            .unwrap();
        let root = doc.get_object(order).unwrap().as_array().unwrap();
        assert_eq!(root[0], root[1]);
    }

    #[test]
    fn content_stream_resolution_handles_nulls_and_reference_chains() {
        let mut doc = Document::with_version("1.5");
        let stream = doc.add_object(lopdf::Stream::new(dictionary! {}, b"q Q".to_vec()));
        let stream_ref = doc.add_object(Object::Reference(stream));
        let array = doc.add_object(Object::Array(vec![
            Object::Null,
            Object::Reference(stream_ref),
        ]));
        let outer = doc.add_object(Object::Reference(array));
        let page = doc.add_object(dictionary! {"Type" => "Page", "Contents" => outer});
        assert_eq!(contents(&doc, page, 64).unwrap(), vec![stream]);
    }
}
