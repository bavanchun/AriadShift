//! Implementation of the `pdf.outline` tool.

use std::collections::{BTreeMap, BTreeSet};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::outline::{OutlineAction, OutlineNode, OutlineParams},
};
use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat};
use serde_json::{Value, json};

use crate::{
    document::{DecodeBudget, PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::{
        InputDoc, OutputDoc, metadata::catalog_object_id, page_set::collect_named_destinations,
    },
    writer,
};

const MAX_TREE_DEPTH: usize = 64;
const MAX_REF_HOPS: usize = 16;
const MAX_OUTLINE_NODES: usize = 100_000;

const PDFDOC_LOW: [char; 32] = [
    '\u{2022}', '\u{2020}', '\u{2021}', '\u{2026}', '\u{2014}', '\u{2013}', '\u{0192}', '\u{2044}',
    '\u{2039}', '\u{203A}', '\u{2212}', '\u{2030}', '\u{201E}', '\u{201C}', '\u{201D}', '\u{2018}',
    '\u{2019}', '\u{201A}', '\u{2122}', '\u{FB01}', '\u{FB02}', '\u{0141}', '\u{0152}', '\u{0160}',
    '\u{0178}', '\u{017D}', '\u{0131}', '\u{0142}', '\u{0153}', '\u{0161}', '\u{017E}', '\u{FFFD}',
];

/// Exports, imports, clears, or generates bookmarks (`/Outlines`) in a PDF document under
/// [`Limits::local()`] reader limits.
pub fn run(
    inputs: &[PdfDoc],
    params: &OutlineParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    run_with_limits(inputs, params, ctx, &Limits::local())
}

/// Runs `pdf.outline` on a raw [`InputDoc`], applying the input's [`Limits`] to the heading
/// reader used by `from_headings` in addition to the limits enforced when the document is opened.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &OutlineParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.outline requires exactly 1 input PDF, got {}",
            inputs.len()
        )));
    }
    let opened = inputs[0].open_for_mutation(ctx)?;
    let outputs = run_with_limits(
        std::slice::from_ref(&opened),
        params,
        ctx,
        &inputs[0].limits,
    )?;
    Ok((vec![opened], outputs))
}

fn run_with_limits(
    inputs: &[PdfDoc],
    params: &OutlineParams,
    ctx: &ToolCtx,
    limits: &Limits,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.outline requires exactly 1 input PDF, got {}",
            inputs.len()
        )));
    }

    let pdf = &inputs[0];
    let stem = naming::doc_stem(pdf).unwrap_or_else(|| "document".to_owned());

    match params.action {
        OutlineAction::Export => {
            if params.json.is_some() {
                return Err(PdfError::InvalidParams(
                    "pdf.outline `json` must not be provided when `action` is `export`".to_owned(),
                ));
            }
            let exported = export_outlines(pdf.inner(), ctx)?;
            let json_val = nodes_to_json(&exported.nodes, 0);
            let out_name = naming::single_output_name(&stem, "outline", "json");
            let output = exported.warnings.into_iter().fold(
                OutputDoc::from_json(out_name, json_val)?,
                |output, warning| output.with_warning(warning),
            );
            Ok(vec![output])
        }
        OutlineAction::Import => {
            let Some(nodes) = &params.json else {
                return Err(PdfError::InvalidParams(
                    "pdf.outline requires `json` bookmark array when `action` is `import`"
                        .to_owned(),
                ));
            };
            let mut out_pdf = pdf.clone();
            let page_count = u32::try_from(out_pdf.inner().get_pages().len()).unwrap_or(0);
            let mut total_nodes = 0usize;
            validate_outline_nodes(nodes, page_count, 0, &mut total_nodes, ctx)?;
            write_outlines(out_pdf.inner_mut()?, nodes, ctx)?;
            Ok(vec![OutputDoc::from_pdf(
                naming::default_pdf_output_name(&stem, "pdf.outline"),
                out_pdf,
            )])
        }
        OutlineAction::Clear => {
            if params.json.is_some() {
                return Err(PdfError::InvalidParams(
                    "pdf.outline `json` must not be provided when `action` is `clear`".to_owned(),
                ));
            }
            let mut out_pdf = pdf.clone();
            clear_outlines(out_pdf.inner_mut()?)?;
            Ok(vec![OutputDoc::from_pdf(
                naming::default_pdf_output_name(&stem, "pdf.outline"),
                out_pdf,
            )])
        }
        OutlineAction::FromHeadings => {
            if params.json.is_some() {
                return Err(PdfError::InvalidParams(
                    "pdf.outline `json` must not be provided when `action` is `from_headings`"
                        .to_owned(),
                ));
            }
            let mut out_pdf = pdf.clone();
            let nodes = detect_heading_outlines(&out_pdf, limits, ctx)?;
            write_outlines(out_pdf.inner_mut()?, &nodes, ctx)?;
            Ok(vec![OutputDoc::from_pdf(
                naming::default_pdf_output_name(&stem, "pdf.outline"),
                out_pdf,
            )])
        }
    }
}

fn nodes_to_json(nodes: &[OutlineNode], depth: usize) -> Value {
    if depth >= MAX_TREE_DEPTH {
        return Value::Array(Vec::new());
    }
    Value::Array(
        nodes
            .iter()
            .map(|n| {
                json!({
                    "title": n.title,
                    "page": n.page,
                    "children": nodes_to_json(&n.children, depth + 1),
                })
            })
            .collect(),
    )
}

fn validate_outline_nodes(
    nodes: &[OutlineNode],
    page_count: u32,
    depth: usize,
    total_nodes: &mut usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH {
        return Err(PdfError::InvalidParams(format!(
            "outline hierarchy exceeds maximum nesting depth of {MAX_TREE_DEPTH}"
        )));
    }
    for node in nodes {
        ctx.check()?;
        *total_nodes = total_nodes.saturating_add(1);
        if *total_nodes > MAX_OUTLINE_NODES {
            return Err(PdfError::InvalidParams(format!(
                "outline hierarchy exceeds maximum node count of {MAX_OUTLINE_NODES}"
            )));
        }
        if node.title.trim().is_empty() {
            return Err(PdfError::InvalidParams(
                "outline node `title` must not be empty".to_owned(),
            ));
        }
        if node.page == 0 || node.page > page_count {
            return Err(PdfError::InvalidParams(format!(
                "outline node {:?} references page {}, which is out of bounds for {page_count}-page document",
                node.title, node.page
            )));
        }
        validate_outline_nodes(&node.children, page_count, depth + 1, total_nodes, ctx)?;
    }
    Ok(())
}

/// Bookmarks read from a document plus warnings about anything the walk had to leave out.
pub(crate) struct ExportedOutlines {
    pub(crate) nodes: Vec<OutlineNode>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Default)]
struct ExportState {
    visited: BTreeSet<ObjectId>,
    node_count: usize,
    last_exported: Option<(String, u32)>,
    depth_cut: Option<(String, u32)>,
    count_cut: Option<(String, u32)>,
}

pub(crate) fn export_outlines(
    doc: &lopdf::Document,
    ctx: &ToolCtx,
) -> Result<ExportedOutlines, PdfError> {
    ctx.check()?;
    let empty = || ExportedOutlines {
        nodes: Vec::new(),
        warnings: Vec::new(),
    };
    let catalog_id = catalog_object_id(doc)?;
    let Ok(Object::Dictionary(catalog)) = doc.get_object(catalog_id) else {
        return Ok(empty());
    };
    let Ok(outlines_obj) = catalog.get(b"Outlines") else {
        return Ok(empty());
    };
    let Some(outlines_dict) = resolve_dict(doc, outlines_obj) else {
        return Ok(empty());
    };

    let page_id_to_num: BTreeMap<ObjectId, u32> = doc
        .get_pages()
        .into_iter()
        .map(|(num, id)| (id, num))
        .collect();
    let named_dests = collect_named_destinations(doc, ctx)?;

    let mut state = ExportState::default();
    let nodes = read_outline_siblings(
        doc,
        outlines_dict.get(b"First").ok(),
        &DestLookup {
            page_id_to_num: &page_id_to_num,
            named_dests: &named_dests,
        },
        &mut state,
        0,
        None,
        ctx,
    )?;

    let mut warnings = Vec::new();
    if let Some((title, page)) = &state.depth_cut {
        warnings.push(format!(
            "outline export truncated: bookmarks nested deeper than {MAX_TREE_DEPTH} levels \
             were not exported (the children of {title:?} on page {page} were dropped)"
        ));
    }
    if let Some((title, page)) = &state.count_cut {
        warnings.push(format!(
            "outline export truncated: stopped after {MAX_OUTLINE_NODES} bookmarks; \
             the bookmarks that follow {title:?} (page {page}) were not exported"
        ));
    }
    Ok(ExportedOutlines { nodes, warnings })
}

/// Page numbers and named destinations used to resolve outline targets.
struct DestLookup<'a> {
    page_id_to_num: &'a BTreeMap<ObjectId, u32>,
    named_dests: &'a BTreeMap<Vec<u8>, Vec<Object>>,
}

fn read_outline_siblings(
    doc: &lopdf::Document,
    first_obj: Option<&Object>,
    lookup: &DestLookup<'_>,
    state: &mut ExportState,
    depth: usize,
    parent: Option<(&str, u32)>,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    if depth >= MAX_TREE_DEPTH {
        if first_obj.and_then(|o| resolve_dict(doc, o)).is_some()
            && state.depth_cut.is_none()
            && let Some((title, page)) = parent
        {
            state.depth_cut = Some((title.to_owned(), page));
        }
        return Ok(Vec::new());
    }
    let mut nodes = Vec::new();
    let mut current = first_obj;

    while let Some(obj) = current {
        ctx.check()?;
        if let Object::Reference(id) = obj
            && !state.visited.insert(*id)
        {
            break;
        }
        let Some(dict) = resolve_dict(doc, obj) else {
            break;
        };
        if state.node_count >= MAX_OUTLINE_NODES {
            if state.count_cut.is_none() {
                state.count_cut = Some(state.last_exported.clone().unwrap_or_default());
            }
            break;
        }
        state.node_count = state.node_count.saturating_add(1);
        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .and_then(decode_pdf_object_string)
            .unwrap_or_default();
        let page = resolve_outline_item_page(doc, dict, lookup, ctx)?.unwrap_or(1);
        state.last_exported = Some((title.clone(), page));
        let children = read_outline_siblings(
            doc,
            dict.get(b"First").ok(),
            lookup,
            state,
            depth + 1,
            Some((&title, page)),
            ctx,
        )?;
        nodes.push(OutlineNode {
            title,
            page,
            children,
        });
        current = dict.get(b"Next").ok();
    }

    Ok(nodes)
}

fn resolve_outline_item_page(
    doc: &lopdf::Document,
    item_dict: &Dictionary,
    lookup: &DestLookup<'_>,
    ctx: &ToolCtx,
) -> Result<Option<u32>, PdfError> {
    if let Ok(dest) = item_dict.get(b"Dest") {
        let mut visited_objs = BTreeSet::new();
        if let Some(page) = resolve_dest_page(doc, dest, lookup, &mut visited_objs, 0, ctx)? {
            return Ok(Some(page));
        }
    }
    if let Ok(action_obj) = item_dict.get(b"A") {
        let mut visited_objs = BTreeSet::new();
        if let Object::Reference(id) = action_obj {
            visited_objs.insert(*id);
        }
        if let Some(action_dict) = resolve_dict(doc, action_obj)
            && let Ok(dest) = action_dict.get(b"D")
        {
            return resolve_dest_page(doc, dest, lookup, &mut visited_objs, 0, ctx);
        }
    }
    Ok(None)
}

fn resolve_dest_page(
    doc: &lopdf::Document,
    dest_obj: &Object,
    lookup: &DestLookup<'_>,
    visited_objs: &mut BTreeSet<ObjectId>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<Option<u32>, PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH {
        return Ok(None);
    }

    let mut resolved = dest_obj;
    for _ in 0..MAX_REF_HOPS {
        match resolved {
            Object::Reference(id) => {
                if !visited_objs.insert(*id) {
                    return Ok(None);
                }
                let Ok(next) = doc.get_object(*id) else {
                    return Ok(None);
                };
                resolved = next;
            }
            _ => break,
        }
    }

    match resolved {
        Object::Array(items) => Ok(dest_array_page(items, lookup.page_id_to_num)),
        Object::Dictionary(dict) => {
            let Ok(d) = dict.get(b"D") else {
                return Ok(None);
            };
            resolve_dest_page(doc, d, lookup, visited_objs, depth + 1, ctx)
        }
        Object::String(name, _) | Object::Name(name) => Ok(lookup
            .named_dests
            .get(name.as_slice())
            .and_then(|items| dest_array_page(items, lookup.page_id_to_num))),
        _ => Ok(None),
    }
}

fn dest_array_page(items: &[Object], page_id_to_num: &BTreeMap<ObjectId, u32>) -> Option<u32> {
    match items.first()? {
        Object::Reference(page_id) => page_id_to_num.get(page_id).copied(),
        Object::Integer(zero_based) if *zero_based >= 0 => u32::try_from(*zero_based)
            .ok()
            .and_then(|n| n.checked_add(1)),
        _ => None,
    }
}

fn clear_outlines(doc: &mut lopdf::Document) -> Result<(), PdfError> {
    let catalog_id = catalog_object_id(doc)?;
    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .as_dict_mut()
        .map_err(|_| PdfError::Malformed)?;
    catalog.remove(b"Outlines");
    Ok(())
}

fn write_outlines(
    doc: &mut lopdf::Document,
    nodes: &[OutlineNode],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if nodes.is_empty() {
        return clear_outlines(doc);
    }

    let pages = doc.get_pages();
    let root_id = doc.new_object_id();
    let (first_id, last_id, total_count) =
        build_outline_siblings(doc, root_id, nodes, &pages, 0, ctx)?;

    let mut root_dict = Dictionary::new();
    root_dict.set("Type", Object::Name(b"Outlines".to_vec()));
    if let Some(first) = first_id {
        root_dict.set("First", Object::Reference(first));
    }
    if let Some(last) = last_id {
        root_dict.set("Last", Object::Reference(last));
    }
    root_dict.set("Count", Object::Integer(total_count as i64));
    doc.objects.insert(root_id, Object::Dictionary(root_dict));

    let catalog_id = catalog_object_id(doc)?;
    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .as_dict_mut()
        .map_err(|_| PdfError::Malformed)?;
    catalog.set("Outlines", Object::Reference(root_id));
    Ok(())
}

fn build_outline_siblings(
    doc: &mut lopdf::Document,
    parent_id: ObjectId,
    siblings: &[OutlineNode],
    pages: &BTreeMap<u32, ObjectId>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<(Option<ObjectId>, Option<ObjectId>, usize), PdfError> {
    ctx.check()?;
    if siblings.is_empty() {
        return Ok((None, None, 0));
    }
    if depth >= MAX_TREE_DEPTH {
        return Err(PdfError::InvalidParams(format!(
            "outline hierarchy exceeds maximum nesting depth of {MAX_TREE_DEPTH}"
        )));
    }

    let ids: Vec<ObjectId> = (0..siblings.len()).map(|_| doc.new_object_id()).collect();
    let mut total_count = siblings.len();

    for (idx, node) in siblings.iter().enumerate() {
        ctx.check()?;
        let item_id = ids[idx];
        let page_id = pages.get(&node.page).copied().ok_or_else(|| {
            PdfError::InvalidParams(format!(
                "outline destination page {} does not exist",
                node.page
            ))
        })?;

        let (child_first, child_last, child_count) =
            build_outline_siblings(doc, item_id, &node.children, pages, depth + 1, ctx)?;
        total_count += child_count;

        let mut dict = Dictionary::new();
        dict.set("Title", encode_pdf_string(&node.title));
        dict.set("Parent", Object::Reference(parent_id));
        dict.set(
            "Dest",
            Object::Array(vec![
                Object::Reference(page_id),
                Object::Name(b"XYZ".to_vec()),
                Object::Integer(0),
                Object::Integer(800),
                Object::Integer(0),
            ]),
        );
        if idx > 0 {
            dict.set("Prev", Object::Reference(ids[idx - 1]));
        }
        if idx + 1 < siblings.len() {
            dict.set("Next", Object::Reference(ids[idx + 1]));
        }
        if let (Some(first), Some(last)) = (child_first, child_last) {
            dict.set("First", Object::Reference(first));
            dict.set("Last", Object::Reference(last));
            dict.set("Count", Object::Integer(child_count as i64));
        }

        doc.objects.insert(item_id, Object::Dictionary(dict));
    }

    Ok((ids.first().copied(), ids.last().copied(), total_count))
}

fn budget_decompressed_limit(budget: DecodeBudget, fallback: u64) -> u64 {
    let mut probe = budget;
    match probe.charge(usize::MAX) {
        Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge { limit })) => limit,
        _ => fallback,
    }
}

fn charge_page_contents(
    doc: &lopdf::Document,
    page_id: ObjectId,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let Ok(page_dict) = doc.get_dictionary(page_id) else {
        return Ok(());
    };
    let Ok(contents_obj) = page_dict.get(b"Contents") else {
        return Ok(());
    };

    let mut streams: Vec<&Stream> = Vec::new();
    match contents_obj {
        Object::Reference(id) => match doc.get_object(*id) {
            Ok(Object::Stream(stream)) => streams.push(stream),
            Ok(Object::Array(arr)) => {
                for item in arr {
                    match item {
                        Object::Reference(sid) => {
                            if let Ok(Object::Stream(s)) = doc.get_object(*sid) {
                                streams.push(s);
                            }
                        }
                        Object::Stream(s) => streams.push(s),
                        _ => {}
                    }
                }
            }
            _ => {}
        },
        Object::Array(arr) => {
            for item in arr {
                match item {
                    Object::Reference(sid) => {
                        if let Ok(Object::Stream(s)) = doc.get_object(*sid) {
                            streams.push(s);
                        }
                    }
                    Object::Stream(s) => streams.push(s),
                    _ => {}
                }
            }
        }
        Object::Stream(stream) => streams.push(stream),
        _ => {}
    }

    for stream in streams {
        ctx.check()?;
        budget.decode(stream)?;
    }
    Ok(())
}

fn detect_heading_outlines(
    pdf: &PdfDoc,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    ctx.check()?;
    let mut budget = pdf.decode_budget();
    for page_id in pdf.inner().page_iter() {
        ctx.check()?;
        charge_page_contents(pdf.inner(), page_id, &mut budget, ctx)?;
    }

    // Every reader limit comes from the caller. The decoded-bytes cap is the tighter per-document
    // budget, and the input-size cap was already enforced when the document was opened (the
    // re-serialised copy below may be slightly larger than the original bytes).
    let mut reader_limits = limits.clone();
    reader_limits.max_decompressed_bytes =
        budget_decompressed_limit(pdf.decode_budget(), limits.max_decompressed_bytes);
    reader_limits.max_input_bytes = None;

    let mut temp_pdf = pdf.clone();
    let bytes = writer::save(&mut temp_pdf, ctx)?;
    ctx.check()?;
    let read_out = ariad_core::reader::pdf::read(&bytes, &reader_limits).map_err(PdfError::from)?;
    ctx.check()?;

    let page_count = u32::try_from(pdf.inner().get_pages().len())
        .unwrap_or(1)
        .max(1);
    let mut raw_headings: Vec<(u8, String, u32)> = Vec::new();

    for (block_idx, block) in read_out.document.body.iter().enumerate() {
        ctx.check()?;
        if let Block::Heading { level, content } = block {
            let title = inlines_to_plain_text(content, 0).trim().to_owned();
            if !title.is_empty() {
                let key = format!("body/{block_idx}");
                let page = read_out
                    .document
                    .layout
                    .as_ref()
                    .and_then(|l| l.blocks.get(&key))
                    .map(|pos| pos.page_no)
                    .unwrap_or(1)
                    .clamp(1, page_count);
                raw_headings.push((*level, title, page));
            }
        }
    }

    if raw_headings.is_empty() {
        for (block_idx, block) in read_out.document.body.iter().enumerate() {
            ctx.check()?;
            if let Block::Paragraph { content } = block {
                let text = inlines_to_plain_text(content, 0).trim().to_owned();
                if let Some(level) = classify_fallback_heading_line(&text) {
                    let key = format!("body/{block_idx}");
                    let page = read_out
                        .document
                        .layout
                        .as_ref()
                        .and_then(|l| l.blocks.get(&key))
                        .map(|pos| pos.page_no)
                        .unwrap_or(1)
                        .clamp(1, page_count);
                    raw_headings.push((level, text, page));
                }
            }
        }
    }

    Ok(build_heading_tree(&raw_headings, 0))
}

fn classify_fallback_heading_line(text: &str) -> Option<u8> {
    if text.is_empty() || text.len() > 120 {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("part ") || lower.starts_with("chapter ") {
        Some(1)
    } else if lower.starts_with("section ") {
        Some(2)
    } else {
        None
    }
}

fn build_heading_tree(items: &[(u8, String, u32)], depth: usize) -> Vec<OutlineNode> {
    if depth >= MAX_TREE_DEPTH {
        return Vec::new();
    }
    let Some(&(first_level, _, _)) = items.first() else {
        return Vec::new();
    };
    let min_level = items
        .iter()
        .map(|(l, _, _)| *l)
        .min()
        .unwrap_or(first_level);
    let mut result = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let (level, ref title, page) = items[i];
        if level == min_level {
            let start_children = i + 1;
            let mut end_children = start_children;
            while end_children < items.len() && items[end_children].0 > min_level {
                end_children += 1;
            }
            let children = build_heading_tree(&items[start_children..end_children], depth + 1);
            result.push(OutlineNode {
                title: title.clone(),
                page,
                children,
            });
            i = end_children;
        } else {
            let mut end_group = i + 1;
            while end_group < items.len() && items[end_group].0 > min_level {
                end_group += 1;
            }
            result.extend(build_heading_tree(&items[i..end_group], depth + 1));
            i = end_group;
        }
    }
    result
}

fn inlines_to_plain_text(inlines: &[Inline], depth: usize) -> String {
    if depth >= MAX_TREE_DEPTH {
        return String::new();
    }
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text } | Inline::Code { text } | Inline::Raw { text, .. } => {
                out.push_str(text);
            }
            Inline::Math { tex, .. } => out.push_str(tex),
            Inline::SoftBreak {} | Inline::LineBreak {} => out.push(' '),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => {
                out.push_str(&inlines_to_plain_text(content, depth + 1));
            }
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::FootnoteRef { id } => out.push_str(id),
        }
    }
    out
}

fn resolve_object<'a>(doc: &'a lopdf::Document, obj: &'a Object) -> Option<&'a Object> {
    let mut current = obj;
    for _ in 0..MAX_REF_HOPS {
        match current {
            Object::Reference(id) => current = doc.get_object(*id).ok()?,
            other => return Some(other),
        }
    }
    None
}

fn resolve_dict<'a>(doc: &'a lopdf::Document, obj: &'a Object) -> Option<&'a Dictionary> {
    resolve_object(doc, obj)?.as_dict().ok()
}

pub(crate) fn encode_pdf_string(text: &str) -> Object {
    if text.is_ascii() {
        Object::String(text.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = Vec::with_capacity(2 + text.len() * 2);
        bytes.extend_from_slice(&[0xFE, 0xFF]);
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

pub(crate) fn decode_pdf_object_string(obj: &Object) -> Option<String> {
    match obj {
        Object::String(bytes, _) => Some(decode_pdf_bytes(bytes)),
        Object::Name(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

pub(crate) fn decode_pdf_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_be_bytes)
            .collect();
        return String::from_utf16_lossy(&units)
            .trim_matches('\0')
            .to_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_le_bytes)
            .collect();
        return String::from_utf16_lossy(&units)
            .trim_matches('\0')
            .to_owned();
    }
    if let Some(stripped) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF])
        && let Ok(s) = std::str::from_utf8(stripped)
    {
        return s.trim_matches('\0').to_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.trim_matches('\0').to_owned();
    }
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7F => b as char,
            0x80..=0x9F => PDFDOC_LOW[(b - 0x80) as usize],
            0xA0..=0xFF => char::from_u32(u32::from(b)).unwrap_or('\u{FFFD}'),
        })
        .collect::<String>()
        .trim_matches('\0')
        .to_owned()
}
