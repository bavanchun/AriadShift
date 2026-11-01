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
    tools::{OutputDoc, metadata::catalog_object_id},
    writer,
};

const MAX_TREE_DEPTH: usize = 64;
const MAX_REF_HOPS: usize = 16;
const MAX_OUTLINE_NODES: usize = 100_000;
const MAX_NAME_TREE_NODES: usize = 10_000;

const PDFDOC_LOW: [char; 32] = [
    '\u{2022}', '\u{2020}', '\u{2021}', '\u{2026}', '\u{2014}', '\u{2013}', '\u{0192}', '\u{2044}',
    '\u{2039}', '\u{203A}', '\u{2212}', '\u{2030}', '\u{201E}', '\u{201C}', '\u{201D}', '\u{2018}',
    '\u{2019}', '\u{201A}', '\u{2122}', '\u{FB01}', '\u{FB02}', '\u{0141}', '\u{0152}', '\u{0160}',
    '\u{0178}', '\u{017D}', '\u{0131}', '\u{0142}', '\u{0153}', '\u{0161}', '\u{017E}', '\u{FFFD}',
];

/// Exports, imports, clears, or generates bookmarks (`/Outlines`) in a PDF document.
pub fn run(
    inputs: &[PdfDoc],
    params: &OutlineParams,
    ctx: &ToolCtx,
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
            let nodes = export_outlines(pdf.inner(), ctx)?;
            let json_val = nodes_to_json(&nodes, 0);
            let out_name = naming::single_output_name(&stem, "outline", "json");
            Ok(vec![OutputDoc::from_json(out_name, json_val)?])
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
            let nodes = detect_heading_outlines(&out_pdf, ctx)?;
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

pub(crate) fn export_outlines(
    doc: &lopdf::Document,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    ctx.check()?;
    let catalog_id = catalog_object_id(doc)?;
    let Ok(Object::Dictionary(catalog)) = doc.get_object(catalog_id) else {
        return Ok(Vec::new());
    };
    let Ok(outlines_obj) = catalog.get(b"Outlines") else {
        return Ok(Vec::new());
    };
    let Some(outlines_dict) = resolve_dict(doc, outlines_obj) else {
        return Ok(Vec::new());
    };

    let page_id_to_num: BTreeMap<ObjectId, u32> = doc
        .get_pages()
        .into_iter()
        .map(|(num, id)| (id, num))
        .collect();

    let mut visited = BTreeSet::new();
    let mut node_count = 0usize;
    read_outline_siblings(
        doc,
        outlines_dict.get(b"First").ok(),
        &page_id_to_num,
        &mut visited,
        0,
        &mut node_count,
        ctx,
    )
}

fn read_outline_siblings(
    doc: &lopdf::Document,
    first_obj: Option<&Object>,
    page_id_to_num: &BTreeMap<ObjectId, u32>,
    visited: &mut BTreeSet<ObjectId>,
    depth: usize,
    node_count: &mut usize,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    if depth >= MAX_TREE_DEPTH {
        return Ok(Vec::new());
    }
    let mut nodes = Vec::new();
    let mut current = first_obj;

    while let Some(obj) = current {
        ctx.check()?;
        if *node_count >= MAX_OUTLINE_NODES {
            break;
        }
        if let Object::Reference(id) = obj
            && !visited.insert(*id)
        {
            break;
        }
        let Some(dict) = resolve_dict(doc, obj) else {
            break;
        };
        *node_count = node_count.saturating_add(1);
        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .and_then(decode_pdf_object_string)
            .unwrap_or_default();
        let page = resolve_outline_item_page(doc, dict, page_id_to_num, ctx)?.unwrap_or(1);
        let children = read_outline_siblings(
            doc,
            dict.get(b"First").ok(),
            page_id_to_num,
            visited,
            depth + 1,
            node_count,
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
    page_id_to_num: &BTreeMap<ObjectId, u32>,
    ctx: &ToolCtx,
) -> Result<Option<u32>, PdfError> {
    if let Ok(dest) = item_dict.get(b"Dest") {
        let mut visited_objs = BTreeSet::new();
        let mut visited_names = BTreeSet::new();
        if let Some(page) = resolve_dest_page(
            doc,
            dest,
            page_id_to_num,
            &mut visited_objs,
            &mut visited_names,
            0,
            ctx,
        )? {
            return Ok(Some(page));
        }
    }
    if let Ok(action_obj) = item_dict.get(b"A") {
        let mut visited_objs = BTreeSet::new();
        let mut visited_names = BTreeSet::new();
        if let Object::Reference(id) = action_obj {
            visited_objs.insert(*id);
        }
        if let Some(action_dict) = resolve_dict(doc, action_obj)
            && let Ok(dest) = action_dict.get(b"D")
        {
            return resolve_dest_page(
                doc,
                dest,
                page_id_to_num,
                &mut visited_objs,
                &mut visited_names,
                0,
                ctx,
            );
        }
    }
    Ok(None)
}

fn resolve_dest_page(
    doc: &lopdf::Document,
    dest_obj: &Object,
    page_id_to_num: &BTreeMap<ObjectId, u32>,
    visited_objs: &mut BTreeSet<ObjectId>,
    visited_names: &mut BTreeSet<Vec<u8>>,
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
        Object::Array(items) => {
            let Some(first) = items.first() else {
                return Ok(None);
            };
            match first {
                Object::Reference(page_id) => Ok(page_id_to_num.get(page_id).copied()),
                Object::Integer(zero_based) if *zero_based >= 0 => Ok(u32::try_from(*zero_based)
                    .ok()
                    .and_then(|n| n.checked_add(1))),
                _ => Ok(None),
            }
        }
        Object::Dictionary(dict) => {
            let Ok(d) = dict.get(b"D") else {
                return Ok(None);
            };
            resolve_dest_page(
                doc,
                d,
                page_id_to_num,
                visited_objs,
                visited_names,
                depth + 1,
                ctx,
            )
        }
        Object::String(name, _) | Object::Name(name) => resolve_named_dest_page(
            doc,
            name,
            page_id_to_num,
            visited_objs,
            visited_names,
            depth + 1,
            ctx,
        ),
        _ => Ok(None),
    }
}

fn resolve_named_dest_page(
    doc: &lopdf::Document,
    name: &[u8],
    page_id_to_num: &BTreeMap<ObjectId, u32>,
    visited_objs: &mut BTreeSet<ObjectId>,
    visited_names: &mut BTreeSet<Vec<u8>>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<Option<u32>, PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH || !visited_names.insert(name.to_vec()) {
        return Ok(None);
    }

    let Ok(catalog_id) = catalog_object_id(doc) else {
        return Ok(None);
    };
    let Some(catalog) = doc
        .get_object(catalog_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
    else {
        return Ok(None);
    };
    if let Ok(dests_obj) = catalog.get(b"Dests")
        && let Some(dests_dict) = resolve_dict(doc, dests_obj)
        && let Ok(target) = dests_dict.get(name)
    {
        return resolve_dest_page(
            doc,
            target,
            page_id_to_num,
            visited_objs,
            visited_names,
            depth + 1,
            ctx,
        );
    }
    if let Ok(names_obj) = catalog.get(b"Names")
        && let Some(names_dict) = resolve_dict(doc, names_obj)
        && let Ok(dests_tree_obj) = names_dict.get(b"Dests")
        && let Some(dests_tree) = resolve_dict(doc, dests_tree_obj)
    {
        let mut visited_tree_nodes = BTreeSet::new();
        if let Object::Reference(root_id) = dests_tree_obj {
            visited_tree_nodes.insert(*root_id);
        }
        let mut visited_count = 0usize;
        if let Some(target) = lookup_name_tree(
            doc,
            dests_tree,
            name,
            &mut visited_tree_nodes,
            &mut visited_count,
            0,
            ctx,
        )? {
            return resolve_dest_page(
                doc,
                target,
                page_id_to_num,
                visited_objs,
                visited_names,
                depth + 1,
                ctx,
            );
        }
    }
    Ok(None)
}

fn lookup_name_tree<'a>(
    doc: &'a lopdf::Document,
    node: &'a Dictionary,
    target_name: &[u8],
    visited_nodes: &mut BTreeSet<ObjectId>,
    visited_count: &mut usize,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<Option<&'a Object>, PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH || *visited_count >= MAX_NAME_TREE_NODES {
        return Ok(None);
    }
    *visited_count = visited_count.saturating_add(1);

    if let Ok(names_obj) = node.get(b"Names")
        && let Some(Object::Array(arr)) = resolve_object(doc, names_obj)
    {
        for [k_obj, v_obj] in arr.as_chunks::<2>().0 {
            if let Some(Object::String(k, _) | Object::Name(k)) = resolve_object(doc, k_obj)
                && k.as_slice() == target_name
            {
                return Ok(Some(v_obj));
            }
        }
    }
    if let Ok(kids_obj) = node.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(doc, kids_obj)
    {
        for kid in kids {
            if let Object::Reference(kid_id) = kid
                && !visited_nodes.insert(*kid_id)
            {
                continue;
            }
            if let Some(kid_dict) = resolve_dict(doc, kid)
                && let Some(found) = lookup_name_tree(
                    doc,
                    kid_dict,
                    target_name,
                    visited_nodes,
                    visited_count,
                    depth + 1,
                    ctx,
                )?
            {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
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

fn budget_decompressed_limit(budget: DecodeBudget) -> u64 {
    let mut probe = budget;
    match probe.charge(usize::MAX) {
        Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge { limit })) => limit,
        _ => Limits::local().max_decompressed_bytes,
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

fn detect_heading_outlines(pdf: &PdfDoc, ctx: &ToolCtx) -> Result<Vec<OutlineNode>, PdfError> {
    ctx.check()?;
    let mut budget = pdf.decode_budget();
    for page_id in pdf.inner().page_iter() {
        ctx.check()?;
        charge_page_contents(pdf.inner(), page_id, &mut budget, ctx)?;
    }

    let mut reader_limits = Limits::local();
    reader_limits.max_decompressed_bytes = budget_decompressed_limit(pdf.decode_budget());

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
