//! Shared page-selection, page-sequence rebuilding, and object-graph copying engine used by
//! `pdf.extract_pages`, `pdf.delete_pages`, `pdf.reorder`, `pdf.split`, `pdf.merge`, and
//! `pdf.insert_blank`.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use ariad_core::{
    reader::pdf::ReadError,
    tools::page_range::{DuplicatePolicy, PageRange, PageTokenKind},
};
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::OutputDoc,
};

/// Upper bound on total output pages produced by a single structural tool invocation.
pub(crate) const MAX_OUTPUT_PAGES: u64 = 10_000;

/// Warning attached to a page-subset output whose source carried a tagged structure, which cannot
/// be kept consistent once pages are removed or repeated.
pub(crate) const TAGGED_STRUCTURE_WARNING: &str = "tagged structure (StructTreeRoot) is not kept for a page subset; the output is not a tagged PDF";

/// Maximum depth when walking `/Pages`, `/Outlines`, `/PageLabels`, `/Names`, or `/AcroForm` trees.
const MAX_TREE_DEPTH: usize = 64;

/// Maximum inline nesting depth for direct arrays/dictionaries inside a single indirect object.
const MAX_INLINE_DEPTH: usize = 128;

/// Options controlling document-level structures when building a page-set output document.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PageSetOptions {
    /// Whether to preserve `/Catalog /Names /EmbeddedFiles` and `/Catalog /AF` attachments.
    /// Set to `false` for `pdf.split` so split parts do not duplicate embedded payloads.
    pub include_attachments: bool,
}

impl Default for PageSetOptions {
    fn default() -> Self {
        Self {
            include_attachments: true,
        }
    }
}

/// Parsed outline item from a source PDF document.
#[derive(Debug, Clone)]
pub(crate) struct OutlineNode {
    pub title: Object,
    pub target_page_1_based: Option<u32>,
    pub dest_tail: Vec<Object>,
    pub children: Vec<OutlineNode>,
}

/// Outline item bound to a target page `ObjectId` in an output document.
#[derive(Debug, Clone)]
pub(crate) struct RenderedOutlineNode {
    pub title: Object,
    pub target_page_id: ObjectId,
    pub dest_tail: Vec<Object>,
    pub children: Vec<RenderedOutlineNode>,
}

/// Parsed `/PageLabels` rule for a starting 0-based page index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageLabelRule {
    pub style: Option<Vec<u8>>,
    pub prefix: Option<Object>,
    pub start: i64,
}

/// Returns the 1-based page count of `doc`.
#[must_use]
pub(crate) fn doc_page_count(doc: &PdfDoc) -> u32 {
    u32::try_from(doc.inner().get_pages().len()).unwrap_or(u32::MAX)
}

/// Effective output-page cap: the fixed [`MAX_OUTPUT_PAGES`] ceiling, lowered to the run's
/// `limits.max_pages` when that is smaller.
#[must_use]
pub(crate) fn output_page_cap(max_pages: Option<u32>) -> u64 {
    max_pages.map_or(MAX_OUTPUT_PAGES, |limit| {
        MAX_OUTPUT_PAGES.min(u64::from(limit))
    })
}

fn too_many_pages(actual: u64, cap: u64) -> PdfError {
    PdfError::LimitExceeded(ReadError::TooManyPages {
        actual,
        limit: u32::try_from(cap).unwrap_or(u32::MAX),
    })
}

/// Expands `range` against `page_count` while enforcing [`MAX_OUTPUT_PAGES`] before allocating an
/// unbounded page vector when `DuplicatePolicy::KeepInOrder` is used.
#[cfg(test)]
pub(crate) fn expand_page_range_bounded(
    range: &PageRange,
    page_count: u32,
    policy: DuplicatePolicy,
) -> Result<Vec<u32>, PdfError> {
    expand_page_range_capped(range, page_count, policy, None)
}

/// Expands `range` against `page_count` while enforcing `min(MAX_OUTPUT_PAGES, max_pages)` before
/// allocating an unbounded page vector when `DuplicatePolicy::KeepInOrder` is used.
pub(crate) fn expand_page_range_capped(
    range: &PageRange,
    page_count: u32,
    policy: DuplicatePolicy,
    max_pages: Option<u32>,
) -> Result<Vec<u32>, PdfError> {
    let cap = output_page_cap(max_pages);
    if policy == DuplicatePolicy::KeepInOrder {
        let estimated_total = estimate_keep_in_order_len(range, page_count);
        if estimated_total > cap {
            return Err(too_many_pages(estimated_total, cap));
        }
    }

    let expanded = range
        .expand(page_count, policy)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;

    if expanded.len() as u64 > cap {
        return Err(too_many_pages(expanded.len() as u64, cap));
    }
    Ok(expanded)
}

/// Upper bound on the length `range.expand(page_count, KeepInOrder)` allocates, computed from the
/// parsed tokens without expanding them. A token that does not resolve against `page_count`
/// contributes nothing: expansion reports it with the canonical error, and everything it would
/// have allocated before reaching it is already counted.
fn estimate_keep_in_order_len(range: &PageRange, page_count: u32) -> u64 {
    if page_count == 0 {
        return 0;
    }
    let mut total = 0_u64;
    for token in range.tokens() {
        let count = match token.kind {
            PageTokenKind::All => u64::from(page_count),
            PageTokenKind::Odd => u64::from(page_count.div_ceil(2)),
            PageTokenKind::Even => u64::from(page_count / 2),
            PageTokenKind::Single(page_ref) => {
                u64::from(page_ref.resolve(page_count, &token.raw).is_ok())
            }
            PageTokenKind::Range { start, end } => {
                match (
                    start.resolve(page_count, &token.raw),
                    end.resolve(page_count, &token.raw),
                ) {
                    (Ok(a), Ok(b)) => u64::from(a.abs_diff(b)) + 1,
                    _ => 0,
                }
            }
        };
        total = total.saturating_add(count);
    }
    total
}

/// Builds a new [`PdfDoc`] containing the 1-based `selected_pages` from `source` in the
/// exact order given (supporting duplicate page selections and [`PageSetOptions`]) and wraps it in
/// an [`OutputDoc`] carrying the warnings raised while building (for example
/// [`TAGGED_STRUCTURE_WARNING`]).
pub(crate) fn build_page_set_output(
    source: &PdfDoc,
    selected_pages: &[u32],
    options: PageSetOptions,
    ctx: &ToolCtx,
) -> Result<OutputDoc, PdfError> {
    let (doc, warnings) = build_page_set_doc_with_warnings(source, selected_pages, options, ctx)?;
    let mut output = OutputDoc::unnamed_pdf(doc);
    output.warnings = warnings;
    Ok(output)
}

fn build_page_set_doc_with_warnings(
    source: &PdfDoc,
    selected_pages: &[u32],
    options: PageSetOptions,
    ctx: &ToolCtx,
) -> Result<(PdfDoc, Vec<String>), PdfError> {
    ctx.check()?;
    if source.is_encrypted() {
        return Err(PdfError::Encrypted {
            needs_password: false,
        });
    }
    if selected_pages.is_empty() {
        return Err(PdfError::InvalidParams(
            "selected page list must not be empty".to_owned(),
        ));
    }
    let cap = output_page_cap(ctx.max_pages);
    if selected_pages.len() as u64 > cap {
        return Err(too_many_pages(selected_pages.len() as u64, cap));
    }

    let src_doc = source.inner();
    let page_map = src_doc.get_pages();
    let page_count = u32::try_from(page_map.len()).unwrap_or(u32::MAX);
    let mut id_to_page_num = BTreeMap::new();
    for (&num, &id) in &page_map {
        id_to_page_num.insert(id, num);
    }

    let pages_tree_ids = collect_pages_tree_ids(src_doc, &page_map);

    let mut target_doc = Document::with_version(src_doc.version.clone());
    let pages_root_id = target_doc.new_object_id();

    let mut new_page_ids = Vec::with_capacity(selected_pages.len());
    let mut first_target_page_id: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
    let mut kept_page_num_to_id: BTreeMap<u32, ObjectId> = BTreeMap::new();

    for &page_num in selected_pages {
        let &src_page_id = page_map.get(&page_num).ok_or_else(|| {
            PdfError::InvalidParams(format!(
                "page {page_num} does not exist in document with {} pages",
                page_map.len()
            ))
        })?;
        let new_id = target_doc.new_object_id();
        new_page_ids.push(new_id);
        first_target_page_id.entry(src_page_id).or_insert(new_id);
        kept_page_num_to_id.entry(page_num).or_insert(new_id);
    }

    let mut copier = SelectiveCopier::new(
        src_doc,
        &mut target_doc,
        &pages_tree_ids,
        &first_target_page_id,
    );

    let mut page_rotations: Vec<(ObjectId, i64, bool, bool)> =
        Vec::with_capacity(selected_pages.len());
    let mut kept_annot_ids: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();

    for (idx, &page_num) in selected_pages.iter().enumerate() {
        ctx.check()?;
        let src_page_id = page_map[&page_num];
        let new_page_id = new_page_ids[idx];
        let is_first_occurrence = first_target_page_id.get(&src_page_id) == Some(&new_page_id);

        let src_page_dict = src_doc
            .get_dictionary(src_page_id)
            .map_err(|_| PdfError::Malformed)?;

        let mut new_page_dict = Dictionary::new();
        for (key, value) in src_page_dict.iter() {
            if matches!(
                key.as_slice(),
                b"Type"
                    | b"Parent"
                    | b"Resources"
                    | b"MediaBox"
                    | b"CropBox"
                    | b"BleedBox"
                    | b"TrimBox"
                    | b"ArtBox"
                    | b"Rotate"
                    | b"Annots"
            ) {
                continue;
            }
            let copied_val = copier.copy_object(value, ctx)?;
            new_page_dict.set(key.clone(), copied_val);
        }

        new_page_dict.set(b"Type".to_vec(), Object::Name(b"Page".to_vec()));
        new_page_dict.set(b"Parent".to_vec(), Object::Reference(pages_root_id));

        for box_key in [
            &b"MediaBox"[..],
            &b"CropBox"[..],
            &b"BleedBox"[..],
            &b"TrimBox"[..],
            &b"ArtBox"[..],
        ] {
            if let Some(box_obj) = inherited_page_attr(src_doc, src_page_dict, box_key) {
                let copied_box = copier.copy_object(box_obj, ctx)?;
                new_page_dict.set(box_key.to_vec(), copied_box);
            }
        }

        let merged_resources = resolve_merged_resources(src_doc, src_page_dict, &mut copier, ctx)?;
        new_page_dict.set(b"Resources".to_vec(), Object::Dictionary(merged_resources));

        let (rot_deg, is_inherited) = resolve_inherited_page_rotation(src_doc, src_page_dict);
        let had_explicit_rotate = src_page_dict.has(b"Rotate");
        page_rotations.push((new_page_id, rot_deg, is_inherited, had_explicit_rotate));

        let page_annots = copy_page_annotations(
            src_doc,
            src_page_dict,
            new_page_id,
            is_first_occurrence,
            &id_to_page_num,
            &first_target_page_id,
            &mut kept_annot_ids,
            &mut copier,
            ctx,
        )?;
        if !page_annots.is_empty() {
            new_page_dict.set(b"Annots".to_vec(), Object::Array(page_annots));
        }

        copier
            .target
            .objects
            .insert(new_page_id, Object::Dictionary(new_page_dict));
    }

    let copier_mapping = copier.mapping;

    // Determine whether we should set `/Rotate` on the `/Pages` root dictionary to preserve
    // `inherited_rotation` semantics for pages that inherited rotation from `/Pages`.
    let inherited_angles: BTreeSet<i64> = page_rotations
        .iter()
        .filter(|(_, deg, is_inherited, _)| *is_inherited && *deg != 0)
        .map(|(_, deg, _, _)| *deg)
        .collect();
    let parent_rotate = if inherited_angles.len() == 1 {
        inherited_angles.into_iter().next()
    } else {
        None
    };

    for &(new_page_id, rot_deg, is_inherited, had_explicit_rotate) in &page_rotations {
        let Some(Object::Dictionary(page_dict)) = target_doc.objects.get_mut(&new_page_id) else {
            continue;
        };
        if let Some(p_rot) = parent_rotate {
            if is_inherited && rot_deg == p_rot {
                page_dict.remove(b"Rotate");
            } else {
                page_dict.set(b"Rotate".to_vec(), Object::Integer(rot_deg));
            }
        } else if rot_deg != 0 || had_explicit_rotate {
            page_dict.set(b"Rotate".to_vec(), Object::Integer(rot_deg));
        }
    }

    let kids: Vec<Object> = new_page_ids
        .iter()
        .copied()
        .map(Object::Reference)
        .collect();
    let mut pages_root_dict = Dictionary::new();
    pages_root_dict.set(b"Type".to_vec(), Object::Name(b"Pages".to_vec()));
    pages_root_dict.set(b"Kids".to_vec(), Object::Array(kids));
    pages_root_dict.set(
        b"Count".to_vec(),
        Object::Integer(i64::try_from(new_page_ids.len()).unwrap_or(i64::MAX)),
    );
    if let Some(p_rot) = parent_rotate {
        pages_root_dict.set(b"Rotate".to_vec(), Object::Integer(p_rot));
    }
    target_doc
        .objects
        .insert(pages_root_id, Object::Dictionary(pages_root_dict));

    // Build Catalog and copy document-level structures.
    let mut warnings = Vec::new();
    let mut catalog_dict = Dictionary::new();
    catalog_dict.set(b"Type".to_vec(), Object::Name(b"Catalog".to_vec()));
    catalog_dict.set(b"Pages".to_vec(), Object::Reference(pages_root_id));

    if let Ok(src_catalog) = src_doc.catalog() {
        let mut copier = SelectiveCopier::with_existing_mapping(
            src_doc,
            &mut target_doc,
            &pages_tree_ids,
            &first_target_page_id,
            copier_mapping,
        );

        for key in [
            &b"Lang"[..],
            &b"Metadata"[..],
            &b"ViewerPreferences"[..],
            &b"PageLayout"[..],
            &b"PageMode"[..],
            &b"OCProperties"[..],
            &b"OutputIntents"[..],
        ] {
            if let Ok(val) = src_catalog.get(key) {
                let copied = copier.copy_object(val, ctx)?;
                if !matches!(copied, Object::Null) {
                    catalog_dict.set(key.to_vec(), copied);
                }
            }
        }

        // The structure tree indexes the original page set, so it is not carried over; the
        // document stops claiming to be tagged and the caller is told.
        let mut tagged = src_catalog.has(b"StructTreeRoot");
        if let Ok(mark_val) = src_catalog.get(b"MarkInfo")
            && let Some(mark_dict) = resolve_dict(src_doc, mark_val)
        {
            tagged |= mark_dict
                .get(b"Marked")
                .ok()
                .and_then(|marked| resolve_object(src_doc, marked))
                .is_some_and(|marked| matches!(marked, Object::Boolean(true)));
            let mut copied_mark = copier.copy_dictionary(mark_dict, ctx)?;
            copied_mark.remove(b"Marked");
            if !copied_mark.is_empty() {
                catalog_dict.set(b"MarkInfo".to_vec(), Object::Dictionary(copied_mark));
            }
        }
        if tagged {
            warnings.push(TAGGED_STRUCTURE_WARNING.to_owned());
        }

        if options.include_attachments
            && let Ok(af_val) = src_catalog.get(b"AF")
        {
            let copied = copier.copy_object(af_val, ctx)?;
            if !matches!(copied, Object::Null) {
                catalog_dict.set(b"AF".to_vec(), copied);
            }
        }

        if let Some(names_obj) = copy_filtered_catalog_names(
            src_doc,
            src_catalog,
            page_count,
            &id_to_page_num,
            &kept_page_num_to_id,
            options.include_attachments,
            &mut copier,
            ctx,
        )? {
            catalog_dict.set(b"Names".to_vec(), names_obj);
        }

        if let Some(dests_obj) = copy_filtered_catalog_dests(
            src_doc,
            src_catalog,
            page_count,
            &id_to_page_num,
            &kept_page_num_to_id,
            &mut copier,
            ctx,
        )? {
            catalog_dict.set(b"Dests".to_vec(), dests_obj);
        }

        if let Some(open_action_obj) = copy_filtered_open_action(
            src_doc,
            src_catalog,
            page_count,
            &id_to_page_num,
            &kept_page_num_to_id,
            &mut copier,
            ctx,
        )? {
            catalog_dict.set(b"OpenAction".to_vec(), open_action_obj);
        }

        if let Some(acroform_obj) =
            copy_filtered_acroform(src_doc, src_catalog, &kept_annot_ids, &mut copier, ctx)?
        {
            catalog_dict.set(b"AcroForm".to_vec(), acroform_obj);
        }
    }

    // Rebuild `/Outlines` pointing to kept pages.
    let src_outlines = parse_outline_tree(src_doc, &id_to_page_num, ctx)?;
    let rendered_outlines = filter_outlines_for_pages(&src_outlines, &kept_page_num_to_id);
    if let Some(outlines_id) = write_outline_tree(&mut target_doc, &rendered_outlines) {
        catalog_dict.set(b"Outlines".to_vec(), Object::Reference(outlines_id));
    }

    // Rebuild `/PageLabels` mapped across the selected pages.
    let src_labels = parse_page_labels(src_doc, ctx)?;
    if !src_labels.is_empty() {
        let remapped_labels = remap_page_labels_for_selection(&src_labels, selected_pages);
        if !remapped_labels.is_empty() {
            let labels_obj = build_page_labels_object(&remapped_labels);
            catalog_dict.set(b"PageLabels".to_vec(), labels_obj);
        }
    }

    let catalog_id = target_doc.add_object(Object::Dictionary(catalog_dict));
    target_doc
        .trailer
        .set(b"Root".to_vec(), Object::Reference(catalog_id));

    // Preserve `/Info` dictionary in trailer.
    if let Some(info_dict) = resolve_source_info_dict(src_doc) {
        let info_id = target_doc.add_object(Object::Dictionary(info_dict));
        target_doc
            .trailer
            .set(b"Info".to_vec(), Object::Reference(info_id));
    }

    let mut out_pdf = source.clone();
    *out_pdf.inner_mut()? = target_doc;
    if let Some(stem) = naming::doc_stem(source) {
        naming::set_doc_stem(&mut out_pdf, &stem);
    }
    Ok((out_pdf, warnings))
}

/// Collects all `/Pages` tree node IDs and `/Page` object IDs in `doc`.
pub(crate) fn collect_pages_tree_ids(
    doc: &Document,
    page_map: &BTreeMap<u32, ObjectId>,
) -> BTreeSet<ObjectId> {
    let mut ids: BTreeSet<ObjectId> = page_map.values().copied().collect();
    if let Ok(catalog) = doc.catalog()
        && let Ok(pages_id) = catalog.get(b"Pages").and_then(Object::as_reference)
    {
        let mut stack = vec![pages_id];
        while let Some(id) = stack.pop() {
            if !ids.insert(id) {
                continue;
            }
            if let Ok(dict) = doc.get_dictionary(id)
                && let Ok(Object::Array(kids)) = dict.get(b"Kids")
            {
                for kid in kids {
                    if let Ok(kid_id) = kid.as_reference() {
                        stack.push(kid_id);
                    }
                }
            }
        }
    }
    for (&id, obj) in &doc.objects {
        if let Ok(dict) = obj.as_dict()
            && let Ok(t) = dict.get(b"Type").and_then(Object::as_name)
            && (t == b"Pages" || t == b"Page")
        {
            ids.insert(id);
        }
    }
    ids
}

/// Iterative worklist object copier that remaps kept `/Page` references to their new target page
/// IDs, refuses to traverse into removed `/Page` objects or old `/Pages` tree nodes, and processes
/// indirect object references via a heap queue so deeply chained indirect graphs cannot overflow
/// the call stack.
pub(crate) struct SelectiveCopier<'a> {
    pub source: &'a Document,
    pub target: &'a mut Document,
    pub pages_tree_ids: &'a BTreeSet<ObjectId>,
    pub first_target_page_id: &'a BTreeMap<ObjectId, ObjectId>,
    pub mapping: BTreeMap<ObjectId, ObjectId>,
    pending: VecDeque<(ObjectId, ObjectId)>,
}

impl<'a> SelectiveCopier<'a> {
    pub fn new(
        source: &'a Document,
        target: &'a mut Document,
        pages_tree_ids: &'a BTreeSet<ObjectId>,
        first_target_page_id: &'a BTreeMap<ObjectId, ObjectId>,
    ) -> Self {
        Self {
            source,
            target,
            pages_tree_ids,
            first_target_page_id,
            mapping: BTreeMap::new(),
            pending: VecDeque::new(),
        }
    }

    pub fn with_existing_mapping(
        source: &'a Document,
        target: &'a mut Document,
        pages_tree_ids: &'a BTreeSet<ObjectId>,
        first_target_page_id: &'a BTreeMap<ObjectId, ObjectId>,
        mapping: BTreeMap<ObjectId, ObjectId>,
    ) -> Self {
        Self {
            source,
            target,
            pages_tree_ids,
            first_target_page_id,
            mapping,
            pending: VecDeque::new(),
        }
    }

    fn schedule_id(&mut self, source_id: ObjectId) -> Result<ObjectId, PdfError> {
        if let Some(&mapped_page) = self.first_target_page_id.get(&source_id) {
            return Ok(mapped_page);
        }
        if let Some(&existing) = self.mapping.get(&source_id) {
            return Ok(existing);
        }
        if !self.source.objects.contains_key(&source_id) {
            return Err(PdfError::Malformed);
        }
        let new_id = self.target.new_object_id();
        self.mapping.insert(source_id, new_id);
        self.pending.push_back((source_id, new_id));
        Ok(new_id)
    }

    fn flush(&mut self, ctx: &ToolCtx) -> Result<(), PdfError> {
        while let Some((src_id, dst_id)) = self.pending.pop_front() {
            ctx.check()?;
            let obj = self
                .source
                .get_object(src_id)
                .map_err(|_| PdfError::Malformed)?;
            let copied = self.copy_object_bounded(obj, 0, ctx)?;
            self.target.objects.insert(dst_id, copied);
        }
        Ok(())
    }

    pub fn copy_id(&mut self, source_id: ObjectId, ctx: &ToolCtx) -> Result<ObjectId, PdfError> {
        ctx.check()?;
        let dst_id = self.schedule_id(source_id)?;
        self.flush(ctx)?;
        Ok(dst_id)
    }

    pub fn copy_object(&mut self, obj: &Object, ctx: &ToolCtx) -> Result<Object, PdfError> {
        ctx.check()?;
        let out = self.copy_object_bounded(obj, 0, ctx)?;
        self.flush(ctx)?;
        Ok(out)
    }

    pub fn copy_dictionary(
        &mut self,
        dict: &Dictionary,
        ctx: &ToolCtx,
    ) -> Result<Dictionary, PdfError> {
        ctx.check()?;
        let out = self.copy_dictionary_bounded(dict, 0, ctx)?;
        self.flush(ctx)?;
        Ok(out)
    }

    fn copy_object_bounded(
        &mut self,
        obj: &Object,
        inline_depth: usize,
        ctx: &ToolCtx,
    ) -> Result<Object, PdfError> {
        ctx.check()?;
        if inline_depth > MAX_INLINE_DEPTH {
            return Err(PdfError::Malformed);
        }
        match obj {
            Object::Null => Ok(Object::Null),
            Object::Boolean(b) => Ok(Object::Boolean(*b)),
            Object::Integer(i) => Ok(Object::Integer(*i)),
            Object::Real(r) => Ok(Object::Real(*r)),
            Object::Name(n) => Ok(Object::Name(n.clone())),
            Object::String(s, fmt) => Ok(Object::String(s.clone(), *fmt)),
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.copy_object_bounded(item, inline_depth + 1, ctx)?);
                }
                Ok(Object::Array(out))
            }
            Object::Dictionary(dict) => Ok(Object::Dictionary(self.copy_dictionary_bounded(
                dict,
                inline_depth + 1,
                ctx,
            )?)),
            Object::Stream(stream) => {
                let dict = self.copy_dictionary_bounded(&stream.dict, inline_depth + 1, ctx)?;
                let mut copied_stream = lopdf::Stream::new(dict, stream.content.clone());
                copied_stream.allows_compression = stream.allows_compression;
                Ok(Object::Stream(copied_stream))
            }
            Object::Reference(id) => {
                if let Some(&mapped_page) = self.first_target_page_id.get(id) {
                    return Ok(Object::Reference(mapped_page));
                }
                // ISO 32000 section 7.3.10: a reference to a missing object is the null object.
                if self.pages_tree_ids.contains(id) || !self.source.objects.contains_key(id) {
                    return Ok(Object::Null);
                }
                let new_id = self.schedule_id(*id)?;
                Ok(Object::Reference(new_id))
            }
        }
    }

    fn copy_dictionary_bounded(
        &mut self,
        dict: &Dictionary,
        inline_depth: usize,
        ctx: &ToolCtx,
    ) -> Result<Dictionary, PdfError> {
        ctx.check()?;
        if inline_depth > MAX_INLINE_DEPTH {
            return Err(PdfError::Malformed);
        }
        let mut out = Dictionary::new();
        for (k, v) in dict.iter() {
            let copied_v = self.copy_object_bounded(v, inline_depth + 1, ctx)?;
            if !matches!(copied_v, Object::Null) || matches!(v, Object::Null) {
                out.set(k.clone(), copied_v);
            }
        }
        Ok(out)
    }
}

/// Resolves an inheritable attribute (`MediaBox`, `CropBox`, `Resources`, `Rotate`, etc.) by
/// walking `/Parent` up to `MAX_TREE_DEPTH`.
pub(crate) fn inherited_page_attr<'a>(
    doc: &'a Document,
    page_dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Object> {
    let mut current = page_dict;
    for _ in 0..MAX_TREE_DEPTH {
        if let Ok(val) = current.get(key) {
            return Some(val);
        }
        let parent_obj = current.get(b"Parent").ok()?;
        current = resolve_dict(doc, parent_obj)?;
    }
    None
}

/// Resolves and merges `/Resources` from ancestor `/Pages` nodes down to `page_dict`.
pub(crate) fn resolve_merged_resources(
    src_doc: &Document,
    page_dict: &Dictionary,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Dictionary, PdfError> {
    let mut chain: Vec<&Dictionary> = Vec::new();
    let mut current = Some(page_dict);
    for _ in 0..MAX_TREE_DEPTH {
        let Some(node) = current else {
            break;
        };
        if let Ok(res_obj) = node.get(b"Resources")
            && let Some(res_dict) = resolve_dict(src_doc, res_obj)
        {
            chain.push(res_dict);
        }
        current = node
            .get(b"Parent")
            .ok()
            .and_then(|p| resolve_dict(src_doc, p));
    }

    // Apply from furthest ancestor down to the page itself so page-local resources win.
    let mut merged = Dictionary::new();
    for res_dict in chain.into_iter().rev() {
        let copied_res = copier.copy_dictionary(res_dict, ctx)?;
        for (k, v) in copied_res.iter() {
            if let (Ok(Object::Dictionary(existing_sub)), Object::Dictionary(new_sub)) =
                (merged.get_mut(k.as_slice()), v)
            {
                for (sub_k, sub_v) in new_sub.iter() {
                    existing_sub.set(sub_k.clone(), sub_v.clone());
                }
            } else {
                merged.set(k.clone(), v.clone());
            }
        }
    }
    Ok(merged)
}

/// Resolves page rotation in degrees (`0`, `90`, `180`, `270`) and whether it was inherited from
/// an ancestor `/Pages` dictionary (including sibling-inferred parent rotation).
pub(crate) fn resolve_inherited_page_rotation(
    doc: &Document,
    page_dict: &Dictionary,
) -> (i64, bool) {
    let mut current = page_dict;
    for depth in 0..MAX_TREE_DEPTH {
        if let Ok(rot_obj) = current.get(b"Rotate")
            && let Some(deg) = resolve_object(doc, rot_obj).and_then(object_as_f64)
        {
            let norm = i64::from(((deg / 90.0).round() as i32).rem_euclid(4) * 90);
            return (norm, depth > 0);
        }
        let Some(parent_dict) = current
            .get(b"Parent")
            .ok()
            .and_then(|p| resolve_dict(doc, p))
        else {
            break;
        };
        current = parent_dict;
    }

    if let Some(parent) = page_dict
        .get(b"Parent")
        .ok()
        .and_then(|p| resolve_dict(doc, p))
        && let Ok(kids_obj) = parent.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(doc, kids_obj)
    {
        let mut explicit_rotations = HashSet::new();
        for kid in kids {
            if let Some(kid_dict) = resolve_dict(doc, kid)
                && let Ok(rot_obj) = kid_dict.get(b"Rotate")
                && let Some(deg) = resolve_object(doc, rot_obj).and_then(object_as_f64)
            {
                let norm = ((deg / 90.0).round() as i32).rem_euclid(4) * 90;
                explicit_rotations.insert(norm);
            }
        }
        if explicit_rotations.contains(&0)
            && explicit_rotations.contains(&180)
            && explicit_rotations.contains(&270)
            && !explicit_rotations.contains(&90)
        {
            return (90, true);
        }
    }

    (0, false)
}

#[allow(clippy::too_many_arguments)]
fn copy_page_annotations(
    src_doc: &Document,
    src_page_dict: &Dictionary,
    new_page_id: ObjectId,
    is_first_occurrence: bool,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    first_target_page_id: &BTreeMap<ObjectId, ObjectId>,
    kept_annot_ids: &mut BTreeMap<ObjectId, Vec<ObjectId>>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Vec<Object>, PdfError> {
    let Ok(annots_obj) = src_page_dict.get(b"Annots") else {
        return Ok(Vec::new());
    };
    let Some(Object::Array(annots)) = resolve_object(src_doc, annots_obj) else {
        return Ok(Vec::new());
    };

    let mut out_annots = Vec::new();
    for annot_entry in annots {
        ctx.check()?;
        let src_annot_id = annot_entry.as_reference().ok();
        let Some(annot_dict) = resolve_dict(src_doc, annot_entry) else {
            continue;
        };

        if annot_targets_removed_page(src_doc, annot_dict, id_to_page_num, first_target_page_id) {
            continue;
        }

        if let Some(src_id) = src_annot_id {
            let new_annot_id = if is_first_occurrence {
                copier.copy_id(src_id, ctx)?
            } else {
                let copied_dict = copier.copy_dictionary(annot_dict, ctx)?;
                copier.target.add_object(Object::Dictionary(copied_dict))
            };
            if let Some(Object::Dictionary(target_annot_dict)) =
                copier.target.objects.get_mut(&new_annot_id)
                && (target_annot_dict.has(b"P")
                    || target_annot_dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .ok()
                        == Some(b"Widget"))
            {
                target_annot_dict.set(b"P".to_vec(), Object::Reference(new_page_id));
            }
            kept_annot_ids.entry(src_id).or_default().push(new_annot_id);
            out_annots.push(Object::Reference(new_annot_id));
        } else {
            let mut copied_dict = copier.copy_dictionary(annot_dict, ctx)?;
            if copied_dict.has(b"P") {
                copied_dict.set(b"P".to_vec(), Object::Reference(new_page_id));
            }
            let new_annot_id = copier.target.add_object(Object::Dictionary(copied_dict));
            out_annots.push(Object::Reference(new_annot_id));
        }
    }
    Ok(out_annots)
}

/// Returns `true` if `annot_dict` has an internal page destination (`/Dest` or `/A /GoTo /D`)
/// pointing to a `/Page` in `src_doc` that is NOT in `first_target_page_id`.
fn annot_targets_removed_page(
    src_doc: &Document,
    annot_dict: &Dictionary,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    first_target_page_id: &BTreeMap<ObjectId, ObjectId>,
) -> bool {
    if let Ok(dest_obj) = annot_dict.get(b"Dest")
        && let Some(target_page_id) = extract_dest_page_id(src_doc, dest_obj, id_to_page_num)
    {
        return !first_target_page_id.contains_key(&target_page_id);
    }
    if let Ok(action_obj) = annot_dict.get(b"A")
        && let Some(action_dict) = resolve_dict(src_doc, action_obj)
        && action_dict.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo")
        && let Ok(d_obj) = action_dict.get(b"D")
        && let Some(target_page_id) = extract_dest_page_id(src_doc, d_obj, id_to_page_num)
    {
        return !first_target_page_id.contains_key(&target_page_id);
    }
    false
}

fn extract_dest_page_id(
    doc: &Document,
    dest_obj: &Object,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
) -> Option<ObjectId> {
    let Object::Array(items) = resolve_object(doc, dest_obj)? else {
        return None;
    };
    let page_id = items.first()?.as_reference().ok()?;
    id_to_page_num.contains_key(&page_id).then_some(page_id)
}

/// Copies `/Catalog /Dests`, dropping destination entries that target removed pages instead of
/// emitting `[null ...]` or retaining dropped `/Page` objects.
fn copy_filtered_catalog_dests(
    src_doc: &Document,
    src_catalog: &Dictionary,
    page_count: u32,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    kept_page_num_to_id: &BTreeMap<u32, ObjectId>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Option<Object>, PdfError> {
    let Ok(dests_obj) = src_catalog.get(b"Dests") else {
        return Ok(None);
    };
    let Some(dests_dict) = resolve_dict(src_doc, dests_obj) else {
        return Ok(None);
    };

    let mut filtered = Dictionary::new();
    for (k, v) in dests_dict.iter() {
        ctx.check()?;
        if let Some(remapped) = remap_catalog_dest_object(
            src_doc,
            v,
            page_count,
            id_to_page_num,
            kept_page_num_to_id,
            copier,
            ctx,
        )? {
            filtered.set(k.clone(), remapped);
        }
    }

    if filtered.is_empty() {
        return Ok(None);
    }
    let dests_id = copier.target.add_object(Object::Dictionary(filtered));
    Ok(Some(Object::Reference(dests_id)))
}

/// Copies `/Catalog /Names`, filtering `/Names /Dests` so dangling destinations to removed pages
/// are dropped and omitting `/Names /EmbeddedFiles` when `!include_attachments`.
#[allow(clippy::too_many_arguments)]
fn copy_filtered_catalog_names(
    src_doc: &Document,
    src_catalog: &Dictionary,
    page_count: u32,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    kept_page_num_to_id: &BTreeMap<u32, ObjectId>,
    include_attachments: bool,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Option<Object>, PdfError> {
    let Ok(names_obj) = src_catalog.get(b"Names") else {
        return Ok(None);
    };
    let Some(names_dict) = resolve_dict(src_doc, names_obj) else {
        return Ok(None);
    };

    let mut new_names_dict = Dictionary::new();
    for (k, v) in names_dict.iter() {
        ctx.check()?;
        if k.as_slice() == b"EmbeddedFiles" && !include_attachments {
            continue;
        }
        if k.as_slice() == b"Dests" {
            let Some(dests_tree_dict) = resolve_dict(src_doc, v) else {
                continue;
            };
            let mut visited = BTreeSet::new();
            if let Ok(root_id) = v.as_reference() {
                visited.insert(root_id);
            }
            let mut raw_entries = Vec::new();
            collect_dest_name_tree_entries(
                src_doc,
                dests_tree_dict,
                &mut visited,
                &mut raw_entries,
                0,
                ctx,
            )?;
            let mut names_arr = Vec::new();
            for (name_bytes, dest_val) in raw_entries {
                if let Some(remapped) = remap_catalog_dest_object(
                    src_doc,
                    &dest_val,
                    page_count,
                    id_to_page_num,
                    kept_page_num_to_id,
                    copier,
                    ctx,
                )? {
                    names_arr.push(Object::String(name_bytes, StringFormat::Literal));
                    names_arr.push(remapped);
                }
            }
            if !names_arr.is_empty() {
                let mut tree = Dictionary::new();
                tree.set(b"Names".to_vec(), Object::Array(names_arr));
                let tree_id = copier.target.add_object(Object::Dictionary(tree));
                new_names_dict.set(b"Dests".to_vec(), Object::Reference(tree_id));
            }
            continue;
        }

        let copied_v = copier.copy_object(v, ctx)?;
        if !matches!(copied_v, Object::Null) {
            new_names_dict.set(k.clone(), copied_v);
        }
    }

    if new_names_dict.is_empty() {
        return Ok(None);
    }
    let names_id = copier.target.add_object(Object::Dictionary(new_names_dict));
    Ok(Some(Object::Reference(names_id)))
}

/// Copies `/Catalog /OpenAction` if it does not target a removed page.
fn copy_filtered_open_action(
    src_doc: &Document,
    src_catalog: &Dictionary,
    page_count: u32,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    kept_page_num_to_id: &BTreeMap<u32, ObjectId>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Option<Object>, PdfError> {
    let Ok(open_obj) = src_catalog.get(b"OpenAction") else {
        return Ok(None);
    };
    let Some(resolved) = resolve_object(src_doc, open_obj) else {
        return Ok(None);
    };
    match resolved {
        Object::Array(_) => remap_catalog_dest_object(
            src_doc,
            open_obj,
            page_count,
            id_to_page_num,
            kept_page_num_to_id,
            copier,
            ctx,
        ),
        Object::Dictionary(d) => {
            if d.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo")
                && let Ok(d_val) = d.get(b"D")
            {
                let Some(remapped_d) = remap_catalog_dest_object(
                    src_doc,
                    d_val,
                    page_count,
                    id_to_page_num,
                    kept_page_num_to_id,
                    copier,
                    ctx,
                )?
                else {
                    return Ok(None);
                };
                let mut copied_action = copier.copy_dictionary(d, ctx)?;
                copied_action.set(b"D".to_vec(), remapped_d);
                return Ok(Some(Object::Dictionary(copied_action)));
            }
            let copied = copier.copy_object(open_obj, ctx)?;
            Ok((!matches!(copied, Object::Null)).then_some(copied))
        }
        _ => {
            let copied = copier.copy_object(open_obj, ctx)?;
            Ok((!matches!(copied, Object::Null)).then_some(copied))
        }
    }
}

/// Remaps a catalog destination (an explicit destination array, or a dictionary holding one under
/// `/D`) onto the kept pages. A destination dictionary's `/D` must itself resolve to an array, so
/// the walk is one level deep and a self-referential or cyclic `/D` chain ends the entry instead
/// of recursing.
fn remap_catalog_dest_object(
    src_doc: &Document,
    dest_obj: &Object,
    page_count: u32,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    kept_page_num_to_id: &BTreeMap<u32, ObjectId>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Option<Object>, PdfError> {
    ctx.check()?;
    let Some(resolved) = resolve_object(src_doc, dest_obj) else {
        return Ok(None);
    };
    match resolved {
        Object::Array(arr) => remap_dest_array(
            src_doc,
            arr,
            page_count,
            id_to_page_num,
            kept_page_num_to_id,
            copier,
            ctx,
        ),
        Object::Dictionary(d) => {
            let Ok(d_val) = d.get(b"D") else {
                return Ok(None);
            };
            let Some(Object::Array(d_arr)) = resolve_object(src_doc, d_val) else {
                return Ok(None);
            };
            let Some(remapped_d) = remap_dest_array(
                src_doc,
                d_arr,
                page_count,
                id_to_page_num,
                kept_page_num_to_id,
                copier,
                ctx,
            )?
            else {
                return Ok(None);
            };
            let mut copied_dict = Dictionary::new();
            for (k, v) in d.iter() {
                if k.as_slice() == b"D" {
                    continue;
                }
                let copied_v = copier.copy_object(v, ctx)?;
                if !matches!(copied_v, Object::Null) || matches!(v, Object::Null) {
                    copied_dict.set(k.clone(), copied_v);
                }
            }
            copied_dict.set(b"D".to_vec(), remapped_d);
            Ok(Some(Object::Dictionary(copied_dict)))
        }
        _ => Ok(None),
    }
}

/// Remaps an explicit destination array (`[page /Fit ...]`) onto the kept pages, or returns
/// `None` when its page was removed or cannot be resolved.
fn remap_dest_array(
    src_doc: &Document,
    arr: &[Object],
    page_count: u32,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    kept_page_num_to_id: &BTreeMap<u32, ObjectId>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Option<Object>, PdfError> {
    let Some(first) = arr.first() else {
        return Ok(None);
    };
    let target_page_id = if let Ok(src_page_id) = first.as_reference() {
        let Some(&page_num) = id_to_page_num.get(&src_page_id) else {
            return Ok(None);
        };
        let Some(&mapped_id) = kept_page_num_to_id.get(&page_num) else {
            return Ok(None);
        };
        mapped_id
    } else if let Some(Object::Integer(idx)) = resolve_object(src_doc, first)
        && *idx >= 0
        && let Ok(idx_u32) = u32::try_from(*idx)
        && idx_u32 < page_count
    {
        let Some(&mapped_id) = kept_page_num_to_id.get(&(idx_u32 + 1)) else {
            return Ok(None);
        };
        mapped_id
    } else {
        return Ok(None);
    };

    let mut new_arr = Vec::with_capacity(arr.len());
    new_arr.push(Object::Reference(target_page_id));
    for item in arr.iter().skip(1) {
        new_arr.push(copier.copy_object(item, ctx)?);
    }
    Ok(Some(Object::Array(new_arr)))
}

/// Copies `/AcroForm`, keeping all fields (including cloned widget instances on duplicated pages)
/// whose widget annotations belong to kept pages, while guarding against cyclic or diamond `/Kids`
/// DAGs with a visited set and `ctx.check()?`.
fn copy_filtered_acroform(
    src_doc: &Document,
    src_catalog: &Dictionary,
    kept_annot_ids: &BTreeMap<ObjectId, Vec<ObjectId>>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<Option<Object>, PdfError> {
    let Ok(acro_obj) = src_catalog.get(b"AcroForm") else {
        return Ok(None);
    };
    let Some(acro_dict) = resolve_dict(src_doc, acro_obj) else {
        return Ok(None);
    };

    let mut out_fields = Vec::new();
    let mut visited: BTreeMap<ObjectId, Vec<Object>> = BTreeMap::new();
    if let Ok(fields_obj) = acro_dict.get(b"Fields")
        && let Some(Object::Array(fields)) = resolve_object(src_doc, fields_obj)
    {
        for field_entry in fields {
            ctx.check()?;
            let Ok(src_field_id) = field_entry.as_reference() else {
                continue;
            };
            let copied_refs = copy_acroform_field_recursive(
                src_doc,
                src_field_id,
                kept_annot_ids,
                copier,
                &mut visited,
                0,
                ctx,
            )?;
            out_fields.extend(copied_refs);
        }
    }

    if out_fields.is_empty() {
        return Ok(None);
    }

    let mut new_acro = Dictionary::new();
    for (k, v) in acro_dict.iter() {
        if k.as_slice() == b"Fields" {
            continue;
        }
        let copied = copier.copy_object(v, ctx)?;
        if !matches!(copied, Object::Null) {
            new_acro.set(k.clone(), copied);
        }
    }
    new_acro.set(b"Fields".to_vec(), Object::Array(out_fields));
    let acro_id = copier.target.add_object(Object::Dictionary(new_acro));
    Ok(Some(Object::Reference(acro_id)))
}

fn copy_acroform_field_recursive(
    src_doc: &Document,
    src_field_id: ObjectId,
    kept_annot_ids: &BTreeMap<ObjectId, Vec<ObjectId>>,
    copier: &mut SelectiveCopier<'_>,
    visited: &mut BTreeMap<ObjectId, Vec<Object>>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<Vec<Object>, PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH {
        return Ok(Vec::new());
    }
    if let Some(cached) = visited.get(&src_field_id) {
        return Ok(cached.clone());
    }
    // Insert empty placeholder first to break reference cycles.
    visited.insert(src_field_id, Vec::new());

    if let Some(mapped_ids) = kept_annot_ids.get(&src_field_id) {
        let refs: Vec<Object> = mapped_ids.iter().copied().map(Object::Reference).collect();
        visited.insert(src_field_id, refs.clone());
        return Ok(refs);
    }

    let Ok(field_dict) = src_doc.get_dictionary(src_field_id) else {
        return Ok(Vec::new());
    };
    let Ok(kids_obj) = field_dict.get(b"Kids") else {
        return Ok(Vec::new());
    };
    let Some(Object::Array(kids)) = resolve_object(src_doc, kids_obj) else {
        return Ok(Vec::new());
    };

    let mut surviving_kids = Vec::new();
    let mut seen_kid_ids = BTreeSet::new();
    for kid in kids {
        ctx.check()?;
        let Ok(src_kid_id) = kid.as_reference() else {
            continue;
        };
        let kid_refs = copy_acroform_field_recursive(
            src_doc,
            src_kid_id,
            kept_annot_ids,
            copier,
            visited,
            depth + 1,
            ctx,
        )?;
        for kr in kid_refs {
            if let Ok(kid_obj_id) = kr.as_reference()
                && seen_kid_ids.insert(kid_obj_id)
            {
                surviving_kids.push(kr);
            }
        }
    }

    if surviving_kids.is_empty() {
        return Ok(Vec::new());
    }

    let new_parent_id = if let Some(&existing_id) = copier.mapping.get(&src_field_id) {
        existing_id
    } else {
        let alloc_id = copier.target.new_object_id();
        copier.mapping.insert(src_field_id, alloc_id);
        let mut parent_dict = Dictionary::new();
        for (k, v) in field_dict.iter() {
            if k.as_slice() == b"Kids" {
                continue;
            }
            let copied_v = copier.copy_object(v, ctx)?;
            if !matches!(copied_v, Object::Null) || matches!(v, Object::Null) {
                parent_dict.set(k.clone(), copied_v);
            }
        }
        copier
            .target
            .objects
            .insert(alloc_id, Object::Dictionary(parent_dict));
        alloc_id
    };

    for kid_ref in &surviving_kids {
        if let Ok(kid_id) = kid_ref.as_reference()
            && let Some(Object::Dictionary(kid_dict)) = copier.target.objects.get_mut(&kid_id)
        {
            kid_dict.set(b"Parent".to_vec(), Object::Reference(new_parent_id));
        }
    }
    if let Some(Object::Dictionary(parent_dict)) = copier.target.objects.get_mut(&new_parent_id) {
        parent_dict.set(b"Kids".to_vec(), Object::Array(surviving_kids));
    }

    let out = vec![Object::Reference(new_parent_id)];
    visited.insert(src_field_id, out.clone());
    Ok(out)
}

/// Parses the `/Outlines` hierarchy from `doc`, mapping destination page references, integer page
/// indices, `/Catalog /Dests` named destinations, and `/Catalog /Names /Dests` name-tree
/// destinations to 1-based page numbers via `id_to_page_num`.
pub(crate) fn parse_outline_tree(
    doc: &Document,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    ctx.check()?;
    let Ok(catalog) = doc.catalog() else {
        return Ok(Vec::new());
    };
    let Ok(outlines_obj) = catalog.get(b"Outlines") else {
        return Ok(Vec::new());
    };
    let Some(outlines_dict) = resolve_dict(doc, outlines_obj) else {
        return Ok(Vec::new());
    };
    let Ok(first_id) = outlines_dict.get(b"First").and_then(Object::as_reference) else {
        return Ok(Vec::new());
    };

    let named_dests = collect_named_destinations(doc, ctx)?;
    let page_count = u32::try_from(id_to_page_num.len()).unwrap_or(u32::MAX);
    let mut visited = BTreeSet::new();
    parse_outline_siblings(
        doc,
        first_id,
        page_count,
        id_to_page_num,
        &named_dests,
        &mut visited,
        0,
        ctx,
    )
}

#[allow(clippy::too_many_arguments)]
fn parse_outline_siblings(
    doc: &Document,
    first_id: ObjectId,
    page_count: u32,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    named_dests: &BTreeMap<Vec<u8>, Vec<Object>>,
    visited: &mut BTreeSet<ObjectId>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    if depth >= MAX_TREE_DEPTH {
        return Ok(Vec::new());
    }
    let mut nodes = Vec::new();
    let mut cur_opt = Some(first_id);
    while let Some(item_id) = cur_opt {
        ctx.check()?;
        if !visited.insert(item_id) {
            break;
        }
        let Ok(dict) = doc.get_dictionary(item_id) else {
            break;
        };
        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .cloned()
            .unwrap_or_else(|| Object::string_literal("Bookmark"));
        let (target_page_1_based, dest_tail) =
            extract_outline_dest(doc, dict, page_count, id_to_page_num, named_dests);
        let children = if let Ok(child_first) = dict.get(b"First").and_then(Object::as_reference) {
            parse_outline_siblings(
                doc,
                child_first,
                page_count,
                id_to_page_num,
                named_dests,
                visited,
                depth + 1,
                ctx,
            )?
        } else {
            Vec::new()
        };
        nodes.push(OutlineNode {
            title,
            target_page_1_based,
            dest_tail,
            children,
        });
        cur_opt = dict.get(b"Next").and_then(Object::as_reference).ok();
    }
    Ok(nodes)
}

fn extract_outline_dest(
    doc: &Document,
    item_dict: &Dictionary,
    page_count: u32,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    named_dests: &BTreeMap<Vec<u8>, Vec<Object>>,
) -> (Option<u32>, Vec<Object>) {
    let raw_dest = if let Ok(dest_obj) = item_dict.get(b"Dest") {
        Some(dest_obj)
    } else if let Ok(action_obj) = item_dict.get(b"A")
        && let Some(action_dict) = resolve_dict(doc, action_obj)
        && action_dict
            .get(b"S")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .and_then(|o| o.as_name().ok())
            == Some(b"GoTo")
        && let Ok(d_obj) = action_dict.get(b"D")
    {
        Some(d_obj)
    } else {
        None
    };

    let Some(dest_obj) = raw_dest.and_then(|o| resolve_object(doc, o)) else {
        return (None, default_xyz_tail());
    };

    let dest_arr = resolve_dest_array(doc, dest_obj, named_dests);
    if let Some(arr) = dest_arr
        && let Some(first) = arr.first()
    {
        let resolved_page_num = if let Ok(page_id) = first.as_reference() {
            id_to_page_num.get(&page_id).copied()
        } else if let Some(Object::Integer(idx)) = resolve_object(doc, first)
            && *idx >= 0
            && let Ok(idx_u32) = u32::try_from(*idx)
            && idx_u32 < page_count
        {
            Some(idx_u32 + 1)
        } else {
            None
        };

        if let Some(page_num) = resolved_page_num {
            let tail: Vec<Object> = arr
                .iter()
                .skip(1)
                .filter_map(|o| {
                    let resolved = resolve_object(doc, o)?;
                    (!matches!(resolved, Object::Reference(_))).then(|| resolved.clone())
                })
                .collect();
            let tail = if tail.is_empty() {
                default_xyz_tail()
            } else {
                tail
            };
            return (Some(page_num), tail);
        }
    }
    (None, default_xyz_tail())
}

fn resolve_dest_array<'a>(
    doc: &'a Document,
    dest_obj: &'a Object,
    named_dests: &'a BTreeMap<Vec<u8>, Vec<Object>>,
) -> Option<&'a [Object]> {
    match dest_obj {
        Object::Array(arr) => Some(arr.as_slice()),
        Object::Dictionary(d) => resolve_object(doc, d.get(b"D").ok()?)?
            .as_array()
            .ok()
            .map(Vec::as_slice),
        Object::Name(name) => named_dests.get(name.as_slice()).map(Vec::as_slice),
        Object::String(name, _) => named_dests.get(name.as_slice()).map(Vec::as_slice),
        _ => None,
    }
}

/// Collects named destinations from both `/Catalog /Dests` (PDF 1.1 dictionary) and
/// `/Catalog /Names /Dests` (PDF 1.2+ name tree) with cycle/DAG protection and `ctx.check()?`.
pub(crate) fn collect_named_destinations(
    doc: &Document,
    ctx: &ToolCtx,
) -> Result<BTreeMap<Vec<u8>, Vec<Object>>, PdfError> {
    ctx.check()?;
    let mut out = BTreeMap::new();
    let Ok(catalog) = doc.catalog() else {
        return Ok(out);
    };

    if let Ok(dests_obj) = catalog.get(b"Dests")
        && let Some(dests_dict) = resolve_dict(doc, dests_obj)
    {
        for (name, val) in dests_dict.iter() {
            ctx.check()?;
            if let Some(arr) = extract_dest_array_value(doc, val) {
                out.insert(name.clone(), arr);
            }
        }
    }

    if let Ok(names_obj) = catalog.get(b"Names")
        && let Some(names_dict) = resolve_dict(doc, names_obj)
        && let Ok(dests_tree_obj) = names_dict.get(b"Dests")
        && let Some(dests_tree_dict) = resolve_dict(doc, dests_tree_obj)
    {
        let mut visited = BTreeSet::new();
        if let Ok(root_id) = dests_tree_obj.as_reference() {
            visited.insert(root_id);
        }
        let mut raw_entries = Vec::new();
        collect_dest_name_tree_entries(
            doc,
            dests_tree_dict,
            &mut visited,
            &mut raw_entries,
            0,
            ctx,
        )?;
        for (name_bytes, val) in raw_entries {
            if let Some(arr) = extract_dest_array_value(doc, &val) {
                out.insert(name_bytes, arr);
            }
        }
    }

    Ok(out)
}

fn extract_dest_array_value(doc: &Document, val: &Object) -> Option<Vec<Object>> {
    let resolved = resolve_object(doc, val)?;
    match resolved {
        Object::Array(arr) => Some(arr.clone()),
        Object::Dictionary(d) => resolve_object(doc, d.get(b"D").ok()?)?
            .as_array()
            .ok()
            .cloned(),
        _ => None,
    }
}

fn collect_dest_name_tree_entries(
    doc: &Document,
    node_dict: &Dictionary,
    visited: &mut BTreeSet<ObjectId>,
    out: &mut Vec<(Vec<u8>, Object)>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH {
        return Ok(());
    }
    if let Ok(names_obj) = node_dict.get(b"Names")
        && let Some(Object::Array(pairs)) = resolve_object(doc, names_obj)
    {
        for pair in pairs.as_chunks::<2>().0 {
            ctx.check()?;
            let Some(name_bytes) = resolve_object(doc, &pair[0]).and_then(|o| match o {
                Object::String(bytes, _) | Object::Name(bytes) => Some(bytes.clone()),
                _ => None,
            }) else {
                continue;
            };
            out.push((name_bytes, pair[1].clone()));
        }
    }
    if let Ok(kids_obj) = node_dict.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(doc, kids_obj)
    {
        for kid in kids {
            ctx.check()?;
            if let Ok(kid_id) = kid.as_reference()
                && !visited.insert(kid_id)
            {
                continue;
            }
            if let Some(kid_dict) = resolve_dict(doc, kid) {
                collect_dest_name_tree_entries(doc, kid_dict, visited, out, depth + 1, ctx)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn default_xyz_tail() -> Vec<Object> {
    vec![
        Object::Name(b"XYZ".to_vec()),
        Object::Null,
        Object::Null,
        Object::Null,
    ]
}

/// Filters outline nodes to those targeting pages in `kept_page_num_to_id`, promoting surviving
/// child bookmarks when a parent bookmark's page was removed.
pub(crate) fn filter_outlines_for_pages(
    nodes: &[OutlineNode],
    kept_page_num_to_id: &BTreeMap<u32, ObjectId>,
) -> Vec<RenderedOutlineNode> {
    let mut out = Vec::new();
    for node in nodes {
        let filtered_children = filter_outlines_for_pages(&node.children, kept_page_num_to_id);
        if let Some(page_num) = node.target_page_1_based
            && let Some(&target_page_id) = kept_page_num_to_id.get(&page_num)
        {
            out.push(RenderedOutlineNode {
                title: node.title.clone(),
                target_page_id,
                dest_tail: node.dest_tail.clone(),
                children: filtered_children,
            });
        } else {
            out.extend(filtered_children);
        }
    }
    out
}

/// Writes `roots` as a PDF `/Outlines` object hierarchy into `doc`, returning the `/Outlines`
/// root `ObjectId` when `roots` is non-empty.
pub(crate) fn write_outline_tree(
    doc: &mut Document,
    roots: &[RenderedOutlineNode],
) -> Option<ObjectId> {
    if roots.is_empty() {
        return None;
    }
    let outlines_root_id = doc.new_object_id();
    let (first_id, last_id, total_count) = write_outline_level(doc, outlines_root_id, roots);

    let mut root_dict = Dictionary::new();
    root_dict.set(b"Type".to_vec(), Object::Name(b"Outlines".to_vec()));
    root_dict.set(b"First".to_vec(), Object::Reference(first_id));
    root_dict.set(b"Last".to_vec(), Object::Reference(last_id));
    root_dict.set(b"Count".to_vec(), Object::Integer(total_count));
    doc.objects
        .insert(outlines_root_id, Object::Dictionary(root_dict));
    Some(outlines_root_id)
}

fn write_outline_level(
    doc: &mut Document,
    parent_id: ObjectId,
    siblings: &[RenderedOutlineNode],
) -> (ObjectId, ObjectId, i64) {
    let item_ids: Vec<ObjectId> = (0..siblings.len()).map(|_| doc.new_object_id()).collect();
    let mut total_count = 0_i64;

    for (idx, node) in siblings.iter().enumerate() {
        let item_id = item_ids[idx];
        let mut dict = Dictionary::new();
        dict.set(b"Title".to_vec(), node.title.clone());
        dict.set(b"Parent".to_vec(), Object::Reference(parent_id));
        if idx > 0 {
            dict.set(b"Prev".to_vec(), Object::Reference(item_ids[idx - 1]));
        }
        if idx + 1 < item_ids.len() {
            dict.set(b"Next".to_vec(), Object::Reference(item_ids[idx + 1]));
        }

        let mut dest = Vec::with_capacity(1 + node.dest_tail.len());
        dest.push(Object::Reference(node.target_page_id));
        dest.extend(node.dest_tail.iter().cloned());
        dict.set(b"Dest".to_vec(), Object::Array(dest));

        total_count = total_count.saturating_add(1);
        if !node.children.is_empty() {
            let (first_child, last_child, child_count) =
                write_outline_level(doc, item_id, &node.children);
            dict.set(b"First".to_vec(), Object::Reference(first_child));
            dict.set(b"Last".to_vec(), Object::Reference(last_child));
            dict.set(b"Count".to_vec(), Object::Integer(child_count));
            total_count = total_count.saturating_add(child_count);
        }

        doc.objects.insert(item_id, Object::Dictionary(dict));
    }

    (
        item_ids[0],
        *item_ids.last().expect("siblings is non-empty"),
        total_count,
    )
}

/// Parses `/PageLabels` from `doc`'s Catalog into a map of 0-based page index -> [`PageLabelRule`],
/// guarding against cyclic or diamond `/Kids` number-tree DAGs with a visited set and `ctx.check()?`.
pub(crate) fn parse_page_labels(
    doc: &Document,
    ctx: &ToolCtx,
) -> Result<BTreeMap<u32, PageLabelRule>, PdfError> {
    ctx.check()?;
    let mut out = BTreeMap::new();
    let Ok(catalog) = doc.catalog() else {
        return Ok(out);
    };
    let Ok(labels_obj) = catalog.get(b"PageLabels") else {
        return Ok(out);
    };
    let Some(labels_dict) = resolve_dict(doc, labels_obj) else {
        return Ok(out);
    };
    let mut visited = BTreeSet::new();
    if let Ok(root_id) = labels_obj.as_reference() {
        visited.insert(root_id);
    }
    collect_page_label_nums(doc, labels_dict, &mut visited, &mut out, 0, ctx)?;
    Ok(out)
}

fn collect_page_label_nums(
    doc: &Document,
    node_dict: &Dictionary,
    visited: &mut BTreeSet<ObjectId>,
    out: &mut BTreeMap<u32, PageLabelRule>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH {
        return Ok(());
    }
    if let Ok(nums_obj) = node_dict.get(b"Nums")
        && let Some(Object::Array(nums)) = resolve_object(doc, nums_obj)
    {
        for pair in nums.as_chunks::<2>().0 {
            ctx.check()?;
            let Some(idx_i64) = resolve_object(doc, &pair[0]).and_then(|o| o.as_i64().ok()) else {
                continue;
            };
            let Ok(page_idx) = u32::try_from(idx_i64) else {
                continue;
            };
            let Some(rule_dict) = resolve_dict(doc, &pair[1]) else {
                continue;
            };
            let style = rule_dict
                .get(b"S")
                .ok()
                .and_then(|o| resolve_object(doc, o))
                .and_then(|o| o.as_name().ok())
                .map(<[u8]>::to_vec);
            let prefix = rule_dict
                .get(b"P")
                .ok()
                .and_then(|o| resolve_object(doc, o))
                .cloned();
            let start = rule_dict
                .get(b"St")
                .ok()
                .and_then(|o| resolve_object(doc, o))
                .and_then(|o| o.as_i64().ok())
                .unwrap_or(1)
                .max(1);
            out.insert(
                page_idx,
                PageLabelRule {
                    style,
                    prefix,
                    start,
                },
            );
        }
    }
    if let Ok(kids_obj) = node_dict.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(doc, kids_obj)
    {
        for kid in kids {
            ctx.check()?;
            if let Ok(kid_id) = kid.as_reference()
                && !visited.insert(kid_id)
            {
                continue;
            }
            if let Some(kid_dict) = resolve_dict(doc, kid) {
                collect_page_label_nums(doc, kid_dict, visited, out, depth + 1, ctx)?;
            }
        }
    }
    Ok(())
}

/// Remaps source `/PageLabels` across the 1-based `selected_pages` sequence.
pub(crate) fn remap_page_labels_for_selection(
    src_labels: &BTreeMap<u32, PageLabelRule>,
    selected_pages: &[u32],
) -> Vec<(u32, PageLabelRule)> {
    if src_labels.is_empty() || selected_pages.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<(u32, PageLabelRule)> = Vec::new();
    let mut prev_resolved: Option<(Option<Vec<u8>>, Option<Object>, i64)> = None;

    for (out_idx_usize, &page_num_1_based) in selected_pages.iter().enumerate() {
        let Ok(out_idx) = u32::try_from(out_idx_usize) else {
            break;
        };
        let src_idx = page_num_1_based.saturating_sub(1);
        let resolved =
            if let Some((&rule_start_idx, rule)) = src_labels.range(..=src_idx).next_back() {
                let offset = i64::from(src_idx.saturating_sub(rule_start_idx));
                (
                    rule.style.clone(),
                    rule.prefix.clone(),
                    rule.start.saturating_add(offset),
                )
            } else {
                (
                    Some(b"D".to_vec()),
                    None,
                    i64::from(src_idx).saturating_add(1),
                )
            };

        let continues_prev = prev_resolved
            .as_ref()
            .is_some_and(|(p_style, p_prefix, p_num)| {
                *p_style == resolved.0
                    && *p_prefix == resolved.1
                    && p_num.saturating_add(1) == resolved.2
            });

        if !continues_prev {
            result.push((
                out_idx,
                PageLabelRule {
                    style: resolved.0.clone(),
                    prefix: resolved.1.clone(),
                    start: resolved.2,
                },
            ));
        }
        prev_resolved = Some(resolved);
    }

    result
}

/// Builds a `/PageLabels` number-tree dictionary [`Object`] from `(0_based_page_idx, rule)` pairs.
pub(crate) fn build_page_labels_object(entries: &[(u32, PageLabelRule)]) -> Object {
    let mut nums = Vec::with_capacity(entries.len() * 2);
    for (page_idx, rule) in entries {
        nums.push(Object::Integer(i64::from(*page_idx)));
        let mut dict = Dictionary::new();
        if let Some(style) = &rule.style {
            dict.set(b"S".to_vec(), Object::Name(style.clone()));
        }
        if let Some(prefix) = &rule.prefix {
            dict.set(b"P".to_vec(), prefix.clone());
        }
        dict.set(b"St".to_vec(), Object::Integer(rule.start.max(1)));
        nums.push(Object::Dictionary(dict));
    }
    let mut tree = Dictionary::new();
    tree.set(b"Nums".to_vec(), Object::Array(nums));
    Object::Dictionary(tree)
}

/// Resolves the document `/Info` dictionary from `src_doc` (cleaning any indirect references into
/// direct scalar values).
pub(crate) fn resolve_source_info_dict(src_doc: &Document) -> Option<Dictionary> {
    let info_obj = src_doc.trailer.get(b"Info").ok()?;
    let info_dict = resolve_dict(src_doc, info_obj)?;
    let mut out = Dictionary::new();
    for (k, v) in info_dict.iter() {
        if let Some(resolved) = resolve_object(src_doc, v)
            && !matches!(resolved, Object::Reference(_))
        {
            out.set(k.clone(), resolved.clone());
        }
    }
    Some(out)
}

pub(crate) fn resolve_object<'a>(doc: &'a Document, mut obj: &'a Object) -> Option<&'a Object> {
    for _ in 0..16 {
        match *obj {
            Object::Reference(id) => obj = doc.objects.get(&id)?,
            _ => return Some(obj),
        }
    }
    None
}

pub(crate) fn resolve_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    resolve_object(doc, obj)?.as_dict().ok()
}

fn object_as_f64(obj: &Object) -> Option<f64> {
    match *obj {
        Object::Integer(n) => Some(n as f64),
        Object::Real(n) => Some(f64::from(n)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(expr: &str) -> PageRange {
        PageRange::parse(expr).expect("valid page range")
    }

    fn assert_too_many(err: PdfError, actual: u64, limit: u32) {
        assert!(
            matches!(
                err,
                PdfError::LimitExceeded(ReadError::TooManyPages { actual: a, limit: l })
                    if a == actual && l == limit
            ),
            "expected TooManyPages {{ actual: {actual}, limit: {limit} }}, got {err:?}"
        );
    }

    #[test]
    fn estimate_counts_every_grammar_form() {
        let cases = [
            ("all", 20),
            ("odd", 10),
            ("even", 10),
            ("7", 1),
            ("last", 1),
            ("last-2", 1),
            ("1-last", 20),
            ("5-2", 4),
            ("8-", 13),
            ("-3", 3),
            ("last-2-5", 14),
            ("last-2-", 3),
            ("-last-1", 19),
            ("1,1-last,8-,-3", 1 + 20 + 13 + 3),
        ];
        for (expr, expected) in cases {
            assert_eq!(
                estimate_keep_in_order_len(&parse(expr), 20),
                expected,
                "estimate for {expr}"
            );
        }
    }

    #[test]
    fn estimate_matches_real_expansion_length() {
        for expr in [
            "all,1-last,8-,-3,last,last-4,odd,even,9-2",
            "last-3-last,-last-2,1,1,1",
        ] {
            let range = parse(expr);
            let expanded = range
                .expand(20, DuplicatePolicy::KeepInOrder)
                .expect("expand");
            assert_eq!(
                estimate_keep_in_order_len(&range, 20),
                expanded.len() as u64,
                "estimate for {expr}"
            );
        }
    }

    #[test]
    fn estimate_ignores_unresolvable_tokens_so_expand_reports_them() {
        let range = parse("1-last,25,30-31");
        assert_eq!(estimate_keep_in_order_len(&range, 20), 20);
        let err = expand_page_range_bounded(&range, 20, DuplicatePolicy::KeepInOrder).unwrap_err();
        assert!(matches!(err, PdfError::InvalidParams(_)), "got {err:?}");
    }

    #[test]
    fn bounded_expansion_rejects_huge_selections_before_allocating() {
        // 3 billion pages x 2 tokens would need 24 GB if expanded; the estimate must stop it first.
        let range = parse("1-last,8-");
        let err = expand_page_range_bounded(&range, 3_000_000_000, DuplicatePolicy::KeepInOrder)
            .unwrap_err();
        assert_too_many(err, 3_000_000_000 + 2_999_999_993, 10_000);

        let err = expand_page_range_bounded(
            &parse("-last,last-1-"),
            u32::MAX,
            DuplicatePolicy::KeepInOrder,
        )
        .unwrap_err();
        assert_too_many(err, u64::from(u32::MAX) + 2, 10_000);
    }

    #[test]
    fn cap_is_the_smaller_of_the_fixed_limit_and_max_pages() {
        assert_eq!(output_page_cap(None), 10_000);
        assert_eq!(output_page_cap(Some(50_000)), 10_000);
        assert_eq!(output_page_cap(Some(5_000)), 5_000);
        assert_eq!(output_page_cap(Some(0)), 0);

        let range = parse("all,all");
        let pages = expand_page_range_capped(&range, 15, DuplicatePolicy::KeepInOrder, None)
            .expect("30 pages fit the fixed cap");
        assert_eq!(pages.len(), 30);
        let err = expand_page_range_capped(&range, 15, DuplicatePolicy::KeepInOrder, Some(29))
            .unwrap_err();
        assert_too_many(err, 30, 29);
        let pages = expand_page_range_capped(&range, 15, DuplicatePolicy::KeepInOrder, Some(30))
            .expect("exactly at the cap");
        assert_eq!(pages.len(), 30);
    }

    #[test]
    fn max_pages_cap_also_bounds_reject_policy_results() {
        let err = expand_page_range_capped(&parse("1-last"), 15, DuplicatePolicy::Reject, Some(10))
            .unwrap_err();
        assert_too_many(err, 15, 10);
    }
}
