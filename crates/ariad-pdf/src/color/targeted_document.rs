//! Copy-on-write document traversal for selective colour transformations.

use std::collections::{HashMap, HashSet};

use ariad_core::{limits::Limits, reader::pdf::ReadError, tools::params::ColorTarget};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content};

use super::{
    dict, page_ids, page_resources, resolve,
    targeted::{self, Color, State, Warnings},
};
use crate::{PdfDoc, PdfError, ToolCtx, document::DecodeBudget, draw::layer};

const MAX_FORM_VISITS: usize = 100_000;
const MAX_FORM_CONTEXTS: usize = 256;

/// Outcome from mapping an image referenced by a selected page.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ImageChange {
    /// Leave it alone without warning.
    Keep,
    /// Replace it with a newly allocated image stream.
    Replaced(ObjectId),
    /// Leave it alone and explain the unsupported encoding.
    Unsupported(&'static str),
}

struct CachedForm {
    state: State,
    resources: Dictionary,
    result: ObjectId,
}

fn resource_size(value: &Object, depth: u16, limit: u16) -> Result<usize, PdfError> {
    if depth > limit {
        return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep { limit }));
    }
    let next = depth.saturating_add(1);
    let size = match value {
        Object::Array(items) => {
            let mut size = items.len().saturating_mul(std::mem::size_of::<Object>());
            for item in items {
                size = size.saturating_add(resource_size(item, next, limit)?);
            }
            size
        }
        Object::Dictionary(items) => dictionary_size(items, next, limit)?,
        Object::Stream(stream) => {
            stream
                .content
                .len()
                .saturating_add(dictionary_size(&stream.dict, next, limit)?)
        }
        Object::String(bytes, _) | Object::Name(bytes) => bytes.len(),
        _ => std::mem::size_of::<Object>(),
    };
    Ok(size)
}

pub(crate) fn dictionary_size(
    items: &Dictionary,
    depth: u16,
    limit: u16,
) -> Result<usize, PdfError> {
    let mut size = 0usize;
    for (key, item) in items {
        size = size
            .saturating_add(key.len())
            .saturating_add(resource_size(item, depth, limit)?);
    }
    Ok(size)
}

struct Walker<'a, F, G> {
    doc: &'a mut Document,
    budget: DecodeBudget,
    limits: &'a Limits,
    ctx: &'a ToolCtx,
    mapper: &'a F,
    target: ColorTarget,
    image: &'a mut G,
    image_cache: HashMap<ObjectId, ImageChange>,
    forms: HashMap<ObjectId, Vec<CachedForm>>,
    active: HashSet<ObjectId>,
    visits: usize,
    warnings: Warnings,
}

impl<F, G> Walker<'_, F, G>
where
    F: Fn([f32; 3]) -> Option<[f32; 3]>,
    G: FnMut(&mut Document, ObjectId, &mut DecodeBudget, &ToolCtx) -> Result<ImageChange, PdfError>,
{
    fn decoded(&mut self, stream: &Stream) -> Result<Vec<u8>, PdfError> {
        self.ctx.check()?;
        if stream.dict.has(b"Filter") {
            self.budget.decode(stream)?.ok_or(PdfError::Malformed)
        } else {
            self.budget.charge(stream.content.len())?;
            Ok(stream.content.clone())
        }
    }

    fn form(
        &mut self,
        id: ObjectId,
        state: &State,
        parent: &Dictionary,
        depth: u16,
    ) -> Result<ObjectId, PdfError> {
        self.ctx.check()?;
        if depth > self.limits.max_nesting_depth {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth,
            }));
        }
        if let Some(contexts) = self.forms.get(&id) {
            if let Some(cached) = contexts
                .iter()
                .find(|entry| entry.state == *state && entry.resources == *parent)
            {
                return Ok(cached.result);
            }
            if contexts.len() >= MAX_FORM_CONTEXTS {
                return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                    limit: MAX_FORM_CONTEXTS,
                }));
            }
        }
        self.visits += 1;
        if self.visits > MAX_FORM_VISITS {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: MAX_FORM_VISITS,
            }));
        }
        if !self.active.insert(id) {
            return Err(PdfError::Malformed);
        }
        let result: Result<ObjectId, PdfError> = (|| {
            let mut stream = self
                .doc
                .get_object(id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?
                .clone();
            let mut resources = match stream.dict.get(b"Resources") {
                Ok(value) => dict(self.doc, value, self.limits.max_nesting_depth)
                    .cloned()
                    .ok_or(PdfError::Malformed)?,
                Err(_) => parent.clone(),
            };
            let mut inherited = state.clone();
            self.copy_inherited_color(parent, &mut resources, &mut inherited.fill)?;
            self.copy_inherited_color(parent, &mut resources, &mut inherited.stroke)?;
            self.warn_type3(&resources);
            let data = self.decoded(&stream)?;
            let content = Content::decode(&data).map_err(|_| PdfError::Malformed)?;
            let content = self.rewrite(&mut resources, &content, &inherited, depth)?;
            let output = content.encode().map_err(|_| PdfError::Malformed)?;
            self.budget.charge(output.len())?;
            stream.dict.set("Resources", Object::Dictionary(resources));
            stream.set_plain_content(output);
            Ok(self.doc.add_object(stream))
        })();
        self.active.remove(&id);
        let result = result?;
        self.budget
            .charge(dictionary_size(parent, 0, self.limits.max_nesting_depth)?)?;
        self.forms.entry(id).or_default().push(CachedForm {
            state: state.clone(),
            resources: parent.clone(),
            result,
        });
        Ok(result)
    }

    fn copy_inherited_color(
        &self,
        parent: &Dictionary,
        resources: &mut Dictionary,
        color: &mut Color,
    ) -> Result<(), PdfError> {
        let Color::Device {
            name: Some(name), ..
        } = color
        else {
            return Ok(());
        };
        if matches!(
            name.as_slice(),
            b"DeviceGray" | b"G" | b"DeviceRGB" | b"RGB" | b"DeviceCMYK" | b"CMYK"
        ) {
            return Ok(());
        }
        let spaces = parent
            .get(b"ColorSpace")
            .ok()
            .and_then(|v| dict(self.doc, v, self.limits.max_nesting_depth))
            .ok_or(PdfError::Malformed)?;
        let value = spaces.get(name).map_err(|_| PdfError::Malformed)?.clone();
        *name = layer::insert_resource(self.doc, resources, b"ColorSpace", "cs", value)?;
        Ok(())
    }

    fn warn_type3(&mut self, resources: &Dictionary) {
        if let Ok(fonts) = resources.get(b"Font")
            && let Some(fonts) = dict(self.doc, fonts, self.limits.max_nesting_depth)
            && fonts.iter().any(|(_, value)| {
                dict(self.doc, value, self.limits.max_nesting_depth).is_some_and(|d| {
                    d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type3")
                })
            })
        {
            targeted::warn(&mut self.warnings, "Type3", "glyph procedures unchanged");
        }
    }

    fn rewrite(
        &mut self,
        resources: &mut Dictionary,
        content: &Content,
        initial: &State,
        depth: u16,
    ) -> Result<Content, PdfError> {
        let mut uses = Vec::new();
        let _ = targeted::rewrite(
            self.doc,
            resources,
            content,
            initial,
            self.target,
            self.limits.max_nesting_depth,
            self.ctx,
            &|_| None,
            |op, state| {
                uses.push((op.clone(), state.clone()));
                Ok(op.clone())
            },
        )?;
        let mut entries = match resources.get(b"XObject") {
            Ok(value) => dict(self.doc, value, self.limits.max_nesting_depth)
                .cloned()
                .ok_or(PdfError::Malformed)?,
            Err(_) => Dictionary::new(),
        };
        let mut next_name = 1usize;
        let mut replacements = Vec::with_capacity(uses.len());
        for (mut op, state) in uses {
            self.ctx.check()?;
            let Some(name) = op.operands.first().and_then(|v| v.as_name().ok()) else {
                return Err(PdfError::Malformed);
            };
            let Some(id) = entries.get(name).ok().and_then(|v| v.as_reference().ok()) else {
                targeted::warn(&mut self.warnings, "Do", "unresolved XObject");
                replacements.push(op);
                continue;
            };
            let subtype = self
                .doc
                .get_object(id)
                .ok()
                .and_then(|v| v.as_stream().ok())
                .and_then(|v| v.dict.get(b"Subtype").ok())
                .and_then(|v| v.as_name().ok())
                .map(<[u8]>::to_vec);
            let replacement = match subtype.as_deref() {
                Some(b"Form") => Some(self.form(id, &state, resources, depth.saturating_add(1))?),
                Some(b"Image") => {
                    let result = if let Some(result) = self.image_cache.get(&id).copied() {
                        result
                    } else {
                        let result = (self.image)(self.doc, id, &mut self.budget, self.ctx)?;
                        self.image_cache.insert(id, result);
                        result
                    };
                    match result {
                        ImageChange::Keep => None,
                        ImageChange::Replaced(id) => Some(id),
                        ImageChange::Unsupported(reason) => {
                            targeted::warn(&mut self.warnings, "image", reason);
                            None
                        }
                    }
                }
                _ => {
                    targeted::warn(&mut self.warnings, "Do", "unsupported XObject");
                    None
                }
            };
            if let Some(id) = replacement {
                let alias = loop {
                    if next_name > targeted::MAX_OPERATIONS {
                        return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                            limit: targeted::MAX_OPERATIONS,
                        }));
                    }
                    let candidate = format!("ASColor{next_name}").into_bytes();
                    next_name += 1;
                    if !entries.has(&candidate) {
                        break candidate;
                    }
                };
                entries.set(alias.clone(), Object::Reference(id));
                op.operands[0] = Object::Name(alias);
            }
            replacements.push(op);
        }
        if !entries.is_empty() {
            resources.set("XObject", Object::Dictionary(entries));
        }
        let mut replacements = replacements.into_iter();
        let (rewritten, warnings) = targeted::rewrite(
            self.doc,
            resources,
            content,
            initial,
            self.target,
            self.limits.max_nesting_depth,
            self.ctx,
            self.mapper,
            |_, _| replacements.next().ok_or(PdfError::Malformed),
        )?;
        for (key, count) in warnings {
            let total = self.warnings.entry(key).or_default();
            *total = total.saturating_add(count);
        }
        Ok(rewritten)
    }
}

/// Rewrite only selected 1-based pages, cloning forms and image resources.
pub(crate) fn rewrite_selected<F, G>(
    pdf: &mut PdfDoc,
    selected: &[u32],
    limits: &Limits,
    ctx: &ToolCtx,
    target: ColorTarget,
    mapper: &F,
    image: &mut G,
) -> Result<Warnings, PdfError>
where
    F: Fn([f32; 3]) -> Option<[f32; 3]>,
    G: FnMut(&mut Document, ObjectId, &mut DecodeBudget, &ToolCtx) -> Result<ImageChange, PdfError>,
{
    let mut working = pdf.clone();
    let budget = working.decode_budget();
    let doc = working.inner_mut()?;
    let pages = page_ids(doc, limits, ctx)?;
    let mut walker = Walker {
        doc,
        budget,
        limits,
        ctx,
        mapper,
        target,
        image,
        image_cache: HashMap::new(),
        forms: HashMap::new(),
        active: HashSet::new(),
        visits: 0,
        warnings: Warnings::new(),
    };
    for page in selected {
        ctx.check()?;
        let index = usize::try_from(page.checked_sub(1).ok_or(PdfError::Malformed)?)
            .map_err(|_| PdfError::Malformed)?;
        let id = *pages.get(index).ok_or(PdfError::Malformed)?;
        let mut resources = page_resources(walker.doc, id, limits)?;
        walker.warn_type3(&resources);
        let page_dict = walker
            .doc
            .get_dictionary(id)
            .map_err(|_| PdfError::Malformed)?;
        if let Ok(annotations) = page_dict.get(b"Annots")
            && resolve(walker.doc, annotations, limits.max_nesting_depth)
                .and_then(|v| v.as_array().ok())
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        dict(walker.doc, item, limits.max_nesting_depth)
                            .is_some_and(|d| d.has(b"AP"))
                    })
                })
        {
            targeted::warn(
                &mut walker.warnings,
                "AP",
                "annotation appearances unchanged",
            );
        }
        let mut bytes = Vec::new();
        for stream_id in walker.doc.get_page_contents(id) {
            let stream = walker
                .doc
                .get_object(stream_id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?
                .clone();
            bytes.extend_from_slice(&walker.decoded(&stream)?);
            bytes.push(b'\n');
        }
        let content = Content::decode(&bytes).map_err(|_| PdfError::Malformed)?;
        let rewritten = walker.rewrite(&mut resources, &content, &State::default(), 0)?;
        let output = rewritten.encode().map_err(|_| PdfError::Malformed)?;
        walker.budget.charge(output.len())?;
        let contents = walker
            .doc
            .add_object(Stream::new(Dictionary::new(), output));
        let page_dict = walker
            .doc
            .get_dictionary_mut(id)
            .map_err(|_| PdfError::Malformed)?;
        page_dict.set("Resources", Object::Dictionary(resources));
        page_dict.set("Contents", contents);
    }
    let warnings = std::mem::take(&mut walker.warnings);
    *pdf = working;
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn document_with_shared_form() -> (PdfDoc, Vec<ObjectId>, ObjectId) {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf-tools/multipage-20.pdf"),
        )
        .unwrap();
        let mut pdf = crate::document::open(&bytes, &Limits::local()).unwrap();
        let pages: Vec<_> = pdf.inner().get_pages().values().take(2).copied().collect();
        let doc = pdf.inner_mut().unwrap();
        let form = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()] }, b"0.2 0.3 0.4 rg 0 0 20 20 re f".to_vec()));
        for page in &pages {
            let content = doc.add_object(Stream::new(dictionary! {}, b"/Fm Do".to_vec()));
            let page = doc.get_dictionary_mut(*page).unwrap();
            page.set(
                "Resources",
                dictionary! { "XObject" => dictionary! { "Fm" => form } },
            );
            page.set("Contents", content);
        }
        (pdf, pages, form)
    }
    fn alias(doc: &Document, page: ObjectId) -> Option<ObjectId> {
        let resources = doc
            .get_dictionary(page)
            .ok()?
            .get(b"Resources")
            .ok()?
            .as_dict()
            .ok()?;
        let objects = resources.get(b"XObject").ok()?.as_dict().ok()?;
        objects.get(b"ASColor1").ok()?.as_reference().ok()
    }
    #[test]
    fn clones_shared_form_only_for_selected_pages() {
        let (mut pdf, pages, original) = document_with_shared_form();
        let original_stream = pdf
            .inner()
            .get_object(original)
            .unwrap()
            .as_stream()
            .unwrap()
            .content
            .clone();
        rewrite_selected(
            &mut pdf,
            &[1],
            &Limits::local(),
            &ToolCtx::new(),
            ColorTarget::All,
            &|_| Some([1.0, 0.0, 0.0]),
            &mut |_, _, _, _| Ok(ImageChange::Keep),
        )
        .unwrap();
        let replacement = alias(pdf.inner(), pages[0]).unwrap();
        assert_ne!(replacement, original);
        assert_eq!(alias(pdf.inner(), pages[1]), None);
        assert_eq!(
            pdf.inner()
                .get_object(original)
                .unwrap()
                .as_stream()
                .unwrap()
                .content,
            original_stream
        );
        let rewritten = pdf
            .inner()
            .get_object(replacement)
            .unwrap()
            .as_stream()
            .unwrap();
        assert!(rewritten.content.windows(2).any(|pair| pair == b"rg"));
    }
    #[test]
    fn shares_cloned_form_for_equivalent_selected_pages() {
        let (mut pdf, pages, _) = document_with_shared_form();
        rewrite_selected(
            &mut pdf,
            &[1, 2],
            &Limits::local(),
            &ToolCtx::new(),
            ColorTarget::All,
            &|_| Some([1.0, 0.0, 0.0]),
            &mut |_, _, _, _| Ok(ImageChange::Keep),
        )
        .unwrap();
        assert_eq!(alias(pdf.inner(), pages[0]), alias(pdf.inner(), pages[1]));
    }
    #[test]
    fn cyclic_forms_fail_transactionally() {
        let (mut pdf, _, form) = document_with_shared_form();
        let stream = pdf
            .inner_mut()
            .unwrap()
            .get_object_mut(form)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        stream.dict.set(
            "Resources",
            dictionary! { "XObject" => dictionary! { "Loop" => form } },
        );
        stream.content = b"/Loop Do".to_vec();
        let before = pdf.inner().objects.clone();
        assert!(
            rewrite_selected(
                &mut pdf,
                &[1],
                &Limits::local(),
                &ToolCtx::new(),
                ColorTarget::All,
                &|_| Some([1.0, 0.0, 0.0]),
                &mut |_, _, _, _| Ok(ImageChange::Keep)
            )
            .is_err()
        );
        assert_eq!(pdf.inner().objects, before);
    }

    #[test]
    fn inherited_icc_state_survives_form_resource_shadowing() {
        let (mut pdf, pages, form) = document_with_shared_form();
        let doc = pdf.inner_mut().unwrap();
        let profile = doc.add_object(Stream::new(dictionary! { "N" => 3 }, vec![]));
        let space = Object::Array(vec![
            Object::Name(b"ICCBased".to_vec()),
            Object::Reference(profile),
        ]);
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"/CS1 cs 0.1 0.2 0.3 sc /Fm Do".to_vec(),
        ));
        let page = doc.get_dictionary_mut(pages[0]).unwrap();
        page.set("Resources", dictionary! { "XObject" => dictionary! { "Fm" => form }, "ColorSpace" => dictionary! { "CS1" => space.clone() } });
        page.set("Contents", content);
        let form_stream = doc.get_object_mut(form).unwrap().as_stream_mut().unwrap();
        form_stream.dict.set(
            "Resources",
            dictionary! { "ColorSpace" => dictionary! { "CS1" => "DeviceGray" } },
        );
        form_stream.content = b"0 0 20 20 re f".to_vec();
        rewrite_selected(
            &mut pdf,
            &[1],
            &Limits::local(),
            &ToolCtx::new(),
            ColorTarget::All,
            &|_| Some([1.0, 0.0, 0.0]),
            &mut |_, _, _, _| Ok(ImageChange::Keep),
        )
        .unwrap();
        let rewritten = pdf
            .inner()
            .get_object(alias(pdf.inner(), pages[0]).unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        let resources = rewritten.dict.get(b"Resources").unwrap().as_dict().unwrap();
        let colors = resources.get(b"ColorSpace").unwrap().as_dict().unwrap();
        assert_eq!(
            colors.get(b"CS1").unwrap().as_name().unwrap(),
            b"DeviceGray"
        );
        assert_eq!(colors.get(b"AScs1").unwrap(), &space);
        let content = Content::decode(&rewritten.content).unwrap();
        assert!(
            content
                .operations
                .iter()
                .any(|op| op.operator == "cs" && op.operands[0].as_name().ok() == Some(b"AScs1"))
        );
    }
}
