//! Implementation of the `pdf.layers` tool.

use std::collections::{BTreeMap, HashSet};

use ariad_core::tools::params::layers::{LayersAction, LayersParams};
use lopdf::{Dictionary, Object, ObjectId, Stream, content::Content, dictionary};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    document::{DecodeBudget, PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::{OutputDoc, metadata::catalog_object_id, outline::decode_pdf_object_string},
};

/// Serializable OCG layer entry emitted by `pdf.layers` (`action = "list"`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LayerEntry {
    /// Human-readable Optional Content Group (`/OCG`) name.
    pub name: String,
    /// Whether this layer is visible by default according to `/OCProperties/D`.
    pub visible: bool,
}

#[derive(Clone, Debug)]
struct ResolvedOcg {
    id: ObjectId,
    name: String,
    visible: bool,
}

/// Lists, toggles default visibility, or removes optional content groups (`/OCProperties`).
pub fn run(
    inputs: &[PdfDoc],
    params: &LayersParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.layers requires exactly 1 input PDF, got {}",
            inputs.len()
        )));
    }

    let pdf = &inputs[0];
    let stem = naming::doc_stem(pdf).unwrap_or_else(|| "document".to_owned());

    match params.action {
        LayersAction::List => {
            if params.names.is_some() || params.visible.is_some() {
                return Err(PdfError::InvalidParams(
                    "pdf.layers `list` does not accept `names` or `visible`".to_owned(),
                ));
            }
            let ocgs = inspect_ocgs(pdf.inner())?;
            let entries: Vec<LayerEntry> = ocgs
                .into_iter()
                .map(|o| LayerEntry {
                    name: o.name,
                    visible: o.visible,
                })
                .collect();
            let out_name = naming::single_output_name(&stem, "layers", "json");
            let json_val: Value =
                serde_json::to_value(&entries).map_err(|_| PdfError::Malformed)?;
            Ok(vec![OutputDoc::from_json(out_name, json_val)?])
        }
        LayersAction::SetDefaultVisibility => {
            let Some(names) = &params.names else {
                return Err(PdfError::InvalidParams(
                    "pdf.layers `set_default_visibility` requires `names`".to_owned(),
                ));
            };
            if names.is_empty() || names.iter().any(|n| n.trim().is_empty()) {
                return Err(PdfError::InvalidParams(
                    "pdf.layers `names` must be non-empty".to_owned(),
                ));
            }
            let Some(visible) = params.visible else {
                return Err(PdfError::InvalidParams(
                    "pdf.layers `set_default_visibility` requires `visible` boolean".to_owned(),
                ));
            };

            let mut out_pdf = pdf.clone();
            set_layers_default_visibility(out_pdf.inner_mut()?, names, visible)?;
            Ok(vec![OutputDoc::from_pdf(
                naming::default_pdf_output_name(&stem, "pdf.layers"),
                out_pdf,
            )])
        }
        LayersAction::Remove => {
            if params.visible.is_some() {
                return Err(PdfError::InvalidParams(
                    "pdf.layers `remove` does not accept `visible`".to_owned(),
                ));
            }
            let Some(names) = &params.names else {
                return Err(PdfError::InvalidParams(
                    "pdf.layers `remove` requires `names`".to_owned(),
                ));
            };
            if names.is_empty() || names.iter().any(|n| n.trim().is_empty()) {
                return Err(PdfError::InvalidParams(
                    "pdf.layers `names` must be non-empty".to_owned(),
                ));
            }

            let mut out_pdf = pdf.clone();
            let mut budget = out_pdf.decode_budget();
            let warnings = remove_layers(out_pdf.inner_mut()?, names, &mut budget, ctx)?;
            let output = OutputDoc::from_pdf(
                naming::default_pdf_output_name(&stem, "pdf.layers"),
                out_pdf,
            );
            Ok(vec![
                warnings
                    .into_iter()
                    .fold(output, |output, warning| output.with_warning(warning)),
            ])
        }
    }
}

fn inspect_ocgs(doc: &lopdf::Document) -> Result<Vec<ResolvedOcg>, PdfError> {
    let catalog_id = catalog_object_id(doc)?;
    let Ok(Object::Dictionary(catalog)) = doc.get_object(catalog_id) else {
        return Ok(Vec::new());
    };
    let Ok(oc_props_obj) = catalog.get(b"OCProperties") else {
        return Ok(Vec::new());
    };
    let Some(oc_props) = resolve_dict(doc, oc_props_obj) else {
        return Ok(Vec::new());
    };
    let Ok(ocgs_obj) = oc_props.get(b"OCGs") else {
        return Ok(Vec::new());
    };
    let Some(Object::Array(ocgs_arr)) = resolve_object(doc, ocgs_obj) else {
        return Ok(Vec::new());
    };

    let (base_on, on_ids, off_ids) = if let Ok(d_obj) = oc_props.get(b"D")
        && let Some(d_dict) = resolve_dict(doc, d_obj)
    {
        let base_state = d_dict
            .get(b"BaseState")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .and_then(|o| o.as_name().ok());
        let base_on = base_state != Some(b"OFF".as_slice());
        let on_ids = extract_ref_set(doc, d_dict.get(b"ON").ok());
        let off_ids = extract_ref_set(doc, d_dict.get(b"OFF").ok());
        (base_on, on_ids, off_ids)
    } else {
        (true, HashSet::new(), HashSet::new())
    };

    let mut result = Vec::new();
    for item in ocgs_arr {
        let Object::Reference(ocg_id) = item else {
            continue;
        };
        let Some(ocg_dict) = resolve_dict(doc, item) else {
            continue;
        };
        let Some(name) = ocg_dict
            .get(b"Name")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .and_then(decode_pdf_object_string)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let visible = if off_ids.contains(ocg_id) {
            false
        } else if on_ids.contains(ocg_id) {
            true
        } else {
            base_on
        };
        result.push(ResolvedOcg {
            id: *ocg_id,
            name,
            visible,
        });
    }

    Ok(result)
}

fn extract_ref_set(doc: &lopdf::Document, arr_obj: Option<&Object>) -> HashSet<ObjectId> {
    let mut set = HashSet::new();
    let Some(obj) = arr_obj else {
        return set;
    };
    if let Some(Object::Array(items)) = resolve_object(doc, obj) {
        for item in items {
            if let Object::Reference(id) = item {
                set.insert(*id);
            }
        }
    }
    set
}

fn set_layers_default_visibility(
    doc: &mut lopdf::Document,
    target_names: &[String],
    visible: bool,
) -> Result<(), PdfError> {
    let ocgs = inspect_ocgs(doc)?;
    for requested in target_names {
        if !ocgs.iter().any(|o| &o.name == requested) {
            return Err(PdfError::InvalidParams(format!(
                "layer {requested:?} not found in document"
            )));
        }
    }

    let target_ids: Vec<ObjectId> = ocgs
        .iter()
        .filter(|o| target_names.contains(&o.name))
        .map(|o| o.id)
        .collect();

    let catalog_id = catalog_object_id(doc)?;
    let mut oc_props = doc
        .get_object(catalog_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|cat| cat.get(b"OCProperties").ok())
        .and_then(|o| resolve_dict(doc, o))
        .cloned()
        .ok_or_else(|| PdfError::InvalidParams("document has no /OCProperties".to_owned()))?;

    let mut d_dict = oc_props
        .get(b"D")
        .ok()
        .and_then(|o| resolve_dict(doc, o))
        .cloned()
        .unwrap_or_else(|| {
            dictionary! {
                "BaseState" => Object::Name(b"ON".to_vec()),
            }
        });

    let mut on_vec = extract_ref_vec(doc, d_dict.get(b"ON").ok());
    let mut off_vec = extract_ref_vec(doc, d_dict.get(b"OFF").ok());

    for id in target_ids {
        if visible {
            off_vec.retain(|&x| x != id);
            if !on_vec.contains(&id) {
                on_vec.push(id);
            }
        } else {
            on_vec.retain(|&x| x != id);
            if !off_vec.contains(&id) {
                off_vec.push(id);
            }
        }
    }

    d_dict.set(
        "ON",
        Object::Array(on_vec.into_iter().map(Object::Reference).collect()),
    );
    d_dict.set(
        "OFF",
        Object::Array(off_vec.into_iter().map(Object::Reference).collect()),
    );
    oc_props.set("D", Object::Dictionary(d_dict));

    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .as_dict_mut()
        .map_err(|_| PdfError::Malformed)?;
    catalog.set("OCProperties", Object::Dictionary(oc_props));
    Ok(())
}

fn extract_ref_vec(doc: &lopdf::Document, arr_obj: Option<&Object>) -> Vec<ObjectId> {
    let mut vec = Vec::new();
    let Some(obj) = arr_obj else {
        return vec;
    };
    if let Some(Object::Array(items)) = resolve_object(doc, obj) {
        for item in items {
            if let Object::Reference(id) = item
                && !vec.contains(id)
            {
                vec.push(*id);
            }
        }
    }
    vec
}

fn remove_layers(
    doc: &mut lopdf::Document,
    target_names: &[String],
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<Vec<String>, PdfError> {
    let ocgs = inspect_ocgs(doc)?;
    for requested in target_names {
        if !ocgs.iter().any(|o| &o.name == requested) {
            return Err(PdfError::InvalidParams(format!(
                "layer {requested:?} not found in document"
            )));
        }
    }

    let removed_ids: HashSet<ObjectId> = ocgs
        .iter()
        .filter(|o| target_names.contains(&o.name))
        .map(|o| o.id)
        .collect();

    let pages: Vec<(u32, ObjectId)> = doc.get_pages().into_iter().collect();
    let mut page_prop_names: Vec<(u32, ObjectId, HashSet<Vec<u8>>)> =
        Vec::with_capacity(pages.len());
    for &(page_no, page_id) in &pages {
        ctx.check()?;
        let names = find_matching_resource_properties(doc, page_id, &removed_ids);
        page_prop_names.push((page_no, page_id, names));
    }

    let mut warnings = Vec::new();
    for (page_no, page_id, prop_names) in &page_prop_names {
        ctx.check()?;
        if prop_names.is_empty() {
            continue;
        }
        if let Some(reason) = strip_page_layer_content(doc, *page_id, prop_names, budget, ctx)? {
            warnings.push(format!(
                "page {page_no}: content stream could not be {reason}; \
                 layer content on this page was left unchanged"
            ));
        }
    }

    for &(_, page_id) in &pages {
        ctx.check()?;
        remove_matching_resource_properties(doc, page_id, &removed_ids);
    }

    let remaining_ids: Vec<ObjectId> = ocgs
        .iter()
        .filter(|o| !removed_ids.contains(&o.id))
        .map(|o| o.id)
        .collect();

    let mut order_filter = OrderFilter::default();
    let catalog_id = catalog_object_id(doc)?;
    if remaining_ids.is_empty() {
        let catalog = doc
            .get_object_mut(catalog_id)
            .map_err(|_| PdfError::Malformed)?
            .as_dict_mut()
            .map_err(|_| PdfError::Malformed)?;
        catalog.remove(b"OCProperties");
    } else {
        let mut oc_props = doc
            .get_object(catalog_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|cat| cat.get(b"OCProperties").ok())
            .and_then(|o| resolve_dict(doc, o))
            .cloned()
            .unwrap_or_else(Dictionary::new);

        oc_props.set(
            "OCGs",
            Object::Array(
                remaining_ids
                    .iter()
                    .copied()
                    .map(Object::Reference)
                    .collect(),
            ),
        );

        if let Some(mut d_dict) = oc_props
            .get(b"D")
            .ok()
            .and_then(|o| resolve_dict(doc, o))
            .cloned()
        {
            clean_oc_config_dict(doc, &mut d_dict, &removed_ids, &mut order_filter, ctx)?;
            oc_props.set("D", Object::Dictionary(d_dict));
        }

        if let Some(Object::Array(configs_arr)) = oc_props
            .get(b"Configs")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .cloned()
        {
            let mut cleaned_configs = Vec::with_capacity(configs_arr.len());
            for cfg_obj in &configs_arr {
                if let Some(mut cfg_dict) = resolve_dict(doc, cfg_obj).cloned() {
                    clean_oc_config_dict(doc, &mut cfg_dict, &removed_ids, &mut order_filter, ctx)?;
                    cleaned_configs.push(Object::Dictionary(cfg_dict));
                }
            }
            oc_props.set("Configs", Object::Array(cleaned_configs));
        }

        let catalog = doc
            .get_object_mut(catalog_id)
            .map_err(|_| PdfError::Malformed)?
            .as_dict_mut()
            .map_err(|_| PdfError::Malformed)?;
        catalog.set("OCProperties", Object::Dictionary(oc_props));
    }

    order_filter.apply(doc);
    for removed_id in removed_ids {
        doc.objects.remove(&removed_id);
    }

    Ok(warnings)
}

fn clean_oc_config_dict(
    doc: &lopdf::Document,
    cfg_dict: &mut Dictionary,
    removed_ids: &HashSet<ObjectId>,
    order_filter: &mut OrderFilter,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    for key in [b"ON".as_slice(), b"OFF".as_slice(), b"Locked".as_slice()] {
        if cfg_dict.has(key) {
            let filtered: Vec<Object> = extract_ref_vec(doc, cfg_dict.get(key).ok())
                .into_iter()
                .filter(|id| !removed_ids.contains(id))
                .map(Object::Reference)
                .collect();
            cfg_dict.set(key.to_vec(), Object::Array(filtered));
        }
    }

    if let Ok(order_obj) = cfg_dict.get(b"Order")
        && let Some(Object::Array(order_arr)) = resolve_object(doc, order_obj)
    {
        let filtered_order = order_filter.filter_array(doc, order_arr, removed_ids, 0, ctx)?;
        cfg_dict.set("Order", Object::Array(filtered_order));
    }

    if let Ok(rb_obj) = cfg_dict.get(b"RBGroups")
        && let Some(Object::Array(rb_arr)) = resolve_object(doc, rb_obj)
    {
        let mut filtered_groups = Vec::new();
        for group_obj in rb_arr {
            if let Some(Object::Array(group)) = resolve_object(doc, group_obj) {
                let kept: Vec<Object> = group
                    .iter()
                    .filter_map(|item| match item {
                        Object::Reference(id) if !removed_ids.contains(id) => {
                            Some(Object::Reference(*id))
                        }
                        _ => None,
                    })
                    .collect();
                if !kept.is_empty() {
                    filtered_groups.push(Object::Array(kept));
                }
            }
        }
        cfg_dict.set("RBGroups", Object::Array(filtered_groups));
    }

    if let Ok(as_obj) = cfg_dict.get(b"AS")
        && let Some(Object::Array(as_arr)) = resolve_object(doc, as_obj)
    {
        let mut filtered_as = Vec::new();
        for item in as_arr {
            if let Some(mut app_dict) = resolve_dict(doc, item).cloned() {
                if app_dict.has(b"OCGs") {
                    let kept: Vec<Object> = extract_ref_vec(doc, app_dict.get(b"OCGs").ok())
                        .into_iter()
                        .filter(|id| !removed_ids.contains(id))
                        .map(Object::Reference)
                        .collect();
                    if kept.is_empty() {
                        continue;
                    }
                    app_dict.set("OCGs", Object::Array(kept));
                }
                filtered_as.push(Object::Dictionary(app_dict));
            }
        }
        cfg_dict.set("AS", Object::Array(filtered_as));
    }
    Ok(())
}

/// Deepest `/Order` nesting that is rewritten; anything below is dropped.
const MAX_ORDER_DEPTH: usize = 64;

/// Rewrites `/Order` trees without expanding shared sub-arrays.
///
/// An indirect array is filtered once, stored back under its own object id when the walk is done,
/// and referenced from every parent that used it. `/Order` is a directed graph, not a tree, so
/// expanding each reference would cost time and output proportional to the number of paths.
#[derive(Default)]
struct OrderFilter {
    filtered: BTreeMap<ObjectId, Vec<Object>>,
    in_progress: HashSet<ObjectId>,
}

impl OrderFilter {
    fn filter_array(
        &mut self,
        doc: &lopdf::Document,
        items: &[Object],
        removed_ids: &HashSet<ObjectId>,
        depth: usize,
        ctx: &ToolCtx,
    ) -> Result<Vec<Object>, PdfError> {
        ctx.check()?;
        if depth > MAX_ORDER_DEPTH {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut has_surviving_ocg = false;

        for (idx, item) in items.iter().enumerate() {
            if idx.is_multiple_of(256) {
                ctx.check()?;
            }
            match item {
                Object::Reference(id) => {
                    if removed_ids.contains(id) {
                        continue;
                    }
                    if let Ok(Object::Array(sub_arr)) = doc.get_object(*id) {
                        if self.keeps_shared_array(
                            doc,
                            *id,
                            sub_arr,
                            removed_ids,
                            depth + 1,
                            ctx,
                        )? {
                            has_surviving_ocg = true;
                            out.push(Object::Reference(*id));
                        }
                    } else {
                        has_surviving_ocg = true;
                        out.push(Object::Reference(*id));
                    }
                }
                Object::Array(sub_arr) => {
                    let nested = self.filter_array(doc, sub_arr, removed_ids, depth + 1, ctx)?;
                    if !nested.is_empty() {
                        has_surviving_ocg = true;
                        out.push(Object::Array(nested));
                    }
                }
                other => {
                    out.push(other.clone());
                }
            }
        }

        if depth > 0 && !has_surviving_ocg {
            Ok(Vec::new())
        } else {
            Ok(out)
        }
    }

    /// Filters the indirect array `id` once and reports whether anything survives in it.
    fn keeps_shared_array(
        &mut self,
        doc: &lopdf::Document,
        id: ObjectId,
        items: &[Object],
        removed_ids: &HashSet<ObjectId>,
        depth: usize,
        ctx: &ToolCtx,
    ) -> Result<bool, PdfError> {
        if depth > MAX_ORDER_DEPTH {
            return Ok(false);
        }
        if let Some(done) = self.filtered.get(&id) {
            return Ok(!done.is_empty());
        }
        if !self.in_progress.insert(id) {
            return Ok(false);
        }
        let result = self.filter_array(doc, items, removed_ids, depth, ctx);
        self.in_progress.remove(&id);
        let filtered = result?;
        let keeps = !filtered.is_empty();
        self.filtered.insert(id, filtered);
        Ok(keeps)
    }

    /// Stores every filtered shared array back into the document.
    fn apply(self, doc: &mut lopdf::Document) {
        for (id, items) in self.filtered {
            doc.objects.insert(id, Object::Array(items));
        }
    }
}

fn decode_page_content_bounded(
    doc: &lopdf::Document,
    page_id: ObjectId,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<Option<Vec<u8>>, PdfError> {
    let Ok(page_dict) = doc.get_dictionary(page_id) else {
        return Ok(Some(Vec::new()));
    };
    let Ok(contents_obj) = page_dict.get(b"Contents") else {
        return Ok(Some(Vec::new()));
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

    let mut combined = Vec::new();
    for (idx, stream) in streams.into_iter().enumerate() {
        ctx.check()?;
        let Some(decoded) = budget.decode(stream)? else {
            return Ok(None);
        };
        if idx > 0 && !combined.ends_with(b"\n") && !combined.ends_with(b"\r") {
            combined.push(b'\n');
        }
        combined.extend_from_slice(&decoded);
    }
    Ok(Some(combined))
}

fn strip_page_layer_content(
    doc: &mut lopdf::Document,
    page_id: ObjectId,
    removed_prop_names: &HashSet<Vec<u8>>,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<Option<&'static str>, PdfError> {
    if removed_prop_names.is_empty() {
        return Ok(None);
    }

    let Some(content_bytes) = decode_page_content_bounded(doc, page_id, budget, ctx)? else {
        return Ok(Some("decoded"));
    };
    if content_bytes.is_empty() {
        return Ok(None);
    }
    let Ok(content) = Content::decode(&content_bytes) else {
        return Ok(Some("parsed"));
    };

    let mut filtered_ops = Vec::with_capacity(content.operations.len());
    let mut skip_depth = 0usize;

    for (idx, op) in content.operations.into_iter().enumerate() {
        if idx.is_multiple_of(256) {
            ctx.check()?;
        }
        if skip_depth > 0 {
            if op.operator == "BDC" || op.operator == "BMC" {
                skip_depth += 1;
            } else if op.operator == "EMC" {
                skip_depth -= 1;
            }
            continue;
        }

        if op.operator == "BDC"
            && op.operands.len() >= 2
            && op.operands[0].as_name().ok() == Some(b"OC".as_slice())
            && let Ok(prop_name) = op.operands[1].as_name()
            && removed_prop_names.contains(prop_name)
        {
            skip_depth = 1;
            continue;
        }

        filtered_ops.push(op);
    }

    let encoded = Content {
        operations: filtered_ops,
    }
    .encode()
    .map_err(|_| PdfError::Malformed)?;

    let new_stream_id = doc.add_object(Object::Stream(Stream::new(dictionary! {}, encoded)));
    if let Ok(Object::Dictionary(page_dict)) = doc.get_object_mut(page_id) {
        page_dict.set("Contents", Object::Reference(new_stream_id));
    }
    Ok(None)
}

fn find_matching_resource_properties(
    doc: &lopdf::Document,
    page_id: ObjectId,
    removed_ids: &HashSet<ObjectId>,
) -> HashSet<Vec<u8>> {
    let mut matching_props = HashSet::new();
    let mut seen_keys = HashSet::new();
    let mut visited_nodes = HashSet::new();
    let mut current_id = Some(page_id);
    let mut depth = 0usize;

    while let Some(node_id) = current_id {
        if depth >= 64 || !visited_nodes.insert(node_id) {
            break;
        }
        depth += 1;
        let Some(node_dict) = doc.get_object(node_id).ok().and_then(|o| o.as_dict().ok()) else {
            break;
        };
        if let Ok(res_obj) = node_dict.get(b"Resources")
            && let Some(res_dict) = resolve_dict(doc, res_obj)
            && let Ok(props_obj) = res_dict.get(b"Properties")
            && let Some(pdict) = resolve_dict(doc, props_obj)
        {
            for (k, v) in pdict.iter() {
                if seen_keys.insert(k.clone())
                    && let Object::Reference(id) = v
                    && removed_ids.contains(id)
                {
                    matching_props.insert(k.clone());
                }
            }
        }
        current_id = node_dict
            .get(b"Parent")
            .ok()
            .and_then(|p| p.as_reference().ok());
    }

    matching_props
}

fn remove_matching_resource_properties(
    doc: &mut lopdf::Document,
    page_id: ObjectId,
    removed_ids: &HashSet<ObjectId>,
) -> HashSet<Vec<u8>> {
    let mut matching_props = HashSet::new();
    let mut visited_nodes = HashSet::new();
    let mut current_id = Some(page_id);
    let mut depth = 0usize;

    while let Some(node_id) = current_id {
        if depth >= 64 || !visited_nodes.insert(node_id) {
            break;
        }
        depth += 1;
        let next_parent = doc
            .get_object(node_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Parent").ok())
            .and_then(|p| p.as_reference().ok());

        clean_node_resource_properties(doc, node_id, removed_ids, &mut matching_props);
        current_id = next_parent;
    }

    matching_props
}

fn clean_node_resource_properties(
    doc: &mut lopdf::Document,
    node_id: ObjectId,
    removed_ids: &HashSet<ObjectId>,
    matching_props: &mut HashSet<Vec<u8>>,
) {
    let res_obj = doc
        .get_object(node_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Resources").ok())
        .cloned();

    let Some(res_obj) = res_obj else {
        return;
    };

    let mut res_dict = match &res_obj {
        Object::Reference(id) => match doc.get_object(*id).ok().and_then(|o| o.as_dict().ok()) {
            Some(d) => d.clone(),
            None => return,
        },
        Object::Dictionary(d) => d.clone(),
        _ => return,
    };

    let mut modified_node = false;
    let props_obj = res_dict.get(b"Properties").ok().cloned();
    if let Some(props_obj) = props_obj {
        let mut props_dict = match &props_obj {
            Object::Reference(id) => doc
                .get_object(*id)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .cloned(),
            Object::Dictionary(d) => Some(d.clone()),
            _ => None,
        };

        if let Some(ref mut pdict) = props_dict {
            let keys_to_remove: Vec<Vec<u8>> = pdict
                .iter()
                .filter_map(|(k, v)| match v {
                    Object::Reference(id) if removed_ids.contains(id) => Some(k.clone()),
                    _ => None,
                })
                .collect();

            if !keys_to_remove.is_empty() {
                modified_node = true;
                for key in keys_to_remove {
                    pdict.remove(&key);
                    matching_props.insert(key);
                }

                if pdict.is_empty() {
                    res_dict.remove(b"Properties");
                } else {
                    res_dict.set("Properties", Object::Dictionary(pdict.clone()));
                }
            }
        }
    }

    if modified_node {
        match res_obj {
            Object::Reference(res_id) => {
                doc.objects.insert(res_id, Object::Dictionary(res_dict));
            }
            Object::Dictionary(_) => {
                if let Ok(Object::Dictionary(node_dict)) = doc.get_object_mut(node_id) {
                    node_dict.set("Resources", Object::Dictionary(res_dict));
                }
            }
            _ => {}
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `levels` shared arrays that each reference the next one twice, ending in `[layer]`.
    fn shared_chain(doc: &mut lopdf::Document, layer: ObjectId, levels: usize) -> ObjectId {
        let mut current = doc.add_object(Object::Array(vec![Object::Reference(layer)]));
        for _ in 1..levels {
            current = doc.add_object(Object::Array(vec![
                Object::Reference(current),
                Object::Reference(current),
            ]));
        }
        current
    }

    #[test]
    fn order_filter_stops_on_a_deep_shared_dag_once_cancelled() {
        let mut doc = lopdf::Document::with_version("1.5");
        let layer = doc.add_object(dictionary! { "Type" => "OCG" });
        let top = shared_chain(&mut doc, layer, 60);
        let items = vec![Object::Reference(top), Object::Reference(top)];

        let ctx = ToolCtx::default();
        ctx.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let result = OrderFilter::default().filter_array(&doc, &items, &HashSet::new(), 0, &ctx);
        assert_eq!(result.unwrap_err(), PdfError::CANCELLED);
    }

    #[test]
    fn order_filter_visits_each_shared_array_once_and_survives_cycles() {
        let mut doc = lopdf::Document::with_version("1.5");
        let layer = doc.add_object(dictionary! { "Type" => "OCG" });
        let top = shared_chain(&mut doc, layer, 60);
        let looped = doc.add_object(Object::Array(Vec::new()));
        doc.objects.insert(
            looped,
            Object::Array(vec![Object::Reference(looped), Object::Reference(layer)]),
        );
        let items = vec![
            Object::Reference(top),
            Object::Reference(top),
            Object::Reference(looped),
        ];

        let mut filter = OrderFilter::default();
        let out = filter
            .filter_array(&doc, &items, &HashSet::new(), 0, &ToolCtx::default())
            .expect("filter");
        assert_eq!(out.len(), 3);
        assert_eq!(filter.filtered.len(), 61);
        assert_eq!(
            filter.filtered[&looped],
            vec![Object::Reference(layer)],
            "a self reference is dropped and the layer is kept"
        );
    }
}
