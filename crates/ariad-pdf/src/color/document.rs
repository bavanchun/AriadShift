use super::{
    ActiveSpace, ColorContext, ColorSpace, MappedColor, UnconvertedColor, dict, resolve, resources,
    rewrite_content_with_state, rewrite_shading, space,
};
use crate::{PdfDoc, PdfError, ToolCtx, document::DecodeBudget};
use ariad_core::{limits::Limits, reader::pdf::ReadError};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content};
use std::collections::{HashMap, HashSet};

const MAX_VISITS: usize = 100_000;
type State = (Option<ActiveSpace>, Option<ActiveSpace>);
type ResourceUses = HashMap<(bool, Vec<u8>), Vec<(State, Vec<usize>)>>;

fn default_state() -> State {
    (
        Some(ActiveSpace::Device(ColorSpace::Gray)),
        Some(ActiveSpace::Device(ColorSpace::Gray)),
    )
}

struct Rewrite<'a, F> {
    doc: &'a mut Document,
    mapper: &'a F,
    limits: &'a Limits,
    ctx: &'a ToolCtx,
    budget: DecodeBudget,
    active: HashSet<ObjectId>,
    visits: usize,
    warnings: Vec<UnconvertedColor>,
}

impl<F> Rewrite<'_, F>
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    fn bytes(&mut self, stream: &Stream) -> Result<Vec<u8>, PdfError> {
        self.ctx.check()?;
        if stream.dict.has(b"Filter") {
            self.budget.decode(stream)?.ok_or(PdfError::Malformed)
        } else {
            self.budget.charge(stream.content.len())?;
            Ok(stream.content.clone())
        }
    }
    fn enter(
        &mut self,
        id: ObjectId,
        parent: &Dictionary,
        depth: u16,
        initial: State,
    ) -> Result<ObjectId, PdfError> {
        if depth > self.limits.max_nesting_depth {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth,
            }));
        }
        self.visits = self.visits.saturating_add(1);
        if self.visits > MAX_VISITS {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: MAX_VISITS,
            }));
        }
        if !self.active.insert(id) {
            return Err(PdfError::Malformed);
        }
        let result = (|| {
            let mut form = self
                .doc
                .get_object(id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?
                .clone();
            let mut resources = form
                .dict
                .get(b"Resources")
                .ok()
                .and_then(|o| dict(self.doc, o, self.limits.max_nesting_depth))
                .cloned()
                .unwrap_or_else(|| parent.clone());
            let bytes = self.bytes(&form)?;
            let mut content = Content::decode(&bytes).map_err(|_| PdfError::Malformed)?;
            self.resources(&mut resources, depth, &mut content, initial)?;
            let mut context = ColorContext {
                limits: self.limits,
                ctx: self.ctx,
                budget: &mut self.budget,
            };
            let warnings = rewrite_content_with_state(
                self.doc,
                &mut content,
                &mut resources,
                self.mapper,
                &mut context,
                initial,
            )?;
            self.warnings.extend(
                warnings
                    .into_iter()
                    .filter(|warning| warning.operator != "sh"),
            );
            let output = content.encode().map_err(|_| PdfError::Malformed)?;
            self.budget.charge(output.len())?;
            form.dict.set("Resources", resources);
            form.set_plain_content(output);
            Ok(self.doc.add_object(form))
        })();
        self.active.remove(&id);
        result
    }
    fn resources(
        &mut self,
        resources: &mut Dictionary,
        depth: u16,
        content: &mut Content,
        initial: State,
    ) -> Result<(), PdfError> {
        let (_, special) = resources::convert(
            self.doc,
            resources,
            self.mapper,
            self.limits,
            &mut self.budget,
        )?;
        let mut state = initial;
        let mut stack = Vec::new();
        let mut uses = ResourceUses::new();
        for (index, op) in content.operations.iter().enumerate() {
            match op.operator.as_str() {
                "q" => stack.push(state),
                "Q" => {
                    if let Some(saved) = stack.pop() {
                        state = saved
                    }
                }
                "g" => state.0 = Some(ActiveSpace::Device(ColorSpace::Gray)),
                "G" => state.1 = Some(ActiveSpace::Device(ColorSpace::Gray)),
                "rg" => state.0 = Some(ActiveSpace::Device(ColorSpace::Rgb)),
                "RG" => state.1 = Some(ActiveSpace::Device(ColorSpace::Rgb)),
                "k" => state.0 = Some(ActiveSpace::Device(ColorSpace::Cmyk)),
                "K" => state.1 = Some(ActiveSpace::Device(ColorSpace::Cmyk)),
                "cs" | "CS" => {
                    let value =
                        op.operands
                            .first()
                            .and_then(|o| o.as_name().ok())
                            .and_then(|name| match special.get(name) {
                                Some(resources::Special::Indexed) => Some(ActiveSpace::Indexed),
                                Some(resources::Special::Pattern(s)) => {
                                    Some(ActiveSpace::Pattern(*s))
                                }
                                None => {
                                    space(self.doc, resources, name, self.limits.max_nesting_depth)
                                        .map(ActiveSpace::Device)
                                }
                            });
                    if op.operator == "cs" {
                        state.0 = value
                    } else {
                        state.1 = value
                    }
                }
                "Do" | "scn" | "SCN" => {
                    if let Some(name) = op.operands.last().and_then(|o| o.as_name().ok()) {
                        let states = uses
                            .entry((op.operator == "Do", name.to_vec()))
                            .or_default();
                        if let Some((_, indexes)) = states.iter_mut().find(|(old, _)| *old == state)
                        {
                            indexes.push(index);
                        } else {
                            states.push((state, vec![index]));
                        }
                    }
                }
                _ => {}
            }
        }
        for category in [b"XObject".as_slice(), b"Pattern"] {
            let used: HashSet<Vec<u8>> = content
                .operations
                .iter()
                .filter_map(|op| {
                    let matches = if category == b"XObject" {
                        op.operator == "Do"
                    } else {
                        op.operator == "scn" || op.operator == "SCN"
                    };
                    matches
                        .then(|| {
                            op.operands
                                .last()
                                .and_then(|o| o.as_name().ok())
                                .map(<[u8]>::to_vec)
                        })
                        .flatten()
                })
                .collect();
            let Some(mut entries) = resources
                .get(category)
                .ok()
                .and_then(|o| dict(self.doc, o, self.limits.max_nesting_depth))
                .cloned()
            else {
                continue;
            };
            let occupied: HashSet<Vec<u8>> = entries.iter().map(|(name, _)| name.clone()).collect();
            let mut additions = Vec::new();
            let mut alias_counter = 0_usize;
            for (name, value) in entries.iter_mut() {
                if !used.contains(name) {
                    continue;
                }
                let Ok(id) = value.as_reference() else {
                    continue;
                };
                let Some(stream) = self
                    .doc
                    .get_object(id)
                    .ok()
                    .and_then(|o| o.as_stream().ok())
                else {
                    continue;
                };
                let is_form = stream.dict.get(b"Subtype").and_then(Object::as_name).ok()
                    == Some(b"Form".as_slice());
                let is_pattern = category == b"Pattern"
                    && stream
                        .dict
                        .get(b"PatternType")
                        .and_then(Object::as_i64)
                        .ok()
                        == Some(1);
                if is_form || is_pattern {
                    let states = uses
                        .get(&(category == b"XObject", name.clone()))
                        .ok_or(PdfError::Malformed)?;
                    for (index, (state, operations)) in states.iter().enumerate() {
                        let cloned = Object::Reference(self.enter(
                            id,
                            resources,
                            depth.saturating_add(1),
                            *state,
                        )?);
                        if index == 0 {
                            *value = cloned;
                            continue;
                        }
                        let alias = loop {
                            let candidate = format!("AriadColor{alias_counter}").into_bytes();
                            alias_counter = alias_counter.saturating_add(1);
                            if !occupied.contains(&candidate)
                                && !additions.iter().any(|(n, _)| *n == candidate)
                            {
                                break candidate;
                            }
                        };
                        for &op in operations {
                            if let Some(last) = content.operations[op].operands.last_mut() {
                                *last = Object::Name(alias.clone());
                            }
                        }
                        additions.push((alias, cloned));
                    }
                }
            }
            for (name, value) in additions {
                entries.set(name, value);
            }
            resources.set(category, Object::Dictionary(entries));
        }
        if let Some(mut shades) = resources
            .get(b"Shading")
            .ok()
            .and_then(|o| dict(self.doc, o, self.limits.max_nesting_depth))
            .cloned()
        {
            let used: HashSet<Vec<u8>> = content
                .operations
                .iter()
                .filter(|op| op.operator == "sh")
                .filter_map(|op| {
                    op.operands
                        .first()
                        .and_then(|v| v.as_name().ok())
                        .map(<[u8]>::to_vec)
                })
                .collect();
            for name in &used {
                if shades.get(name).is_err() {
                    self.warnings.push(UnconvertedColor {
                        operator: "sh".into(),
                        reason: "missing shading resource",
                    });
                }
            }
            for (name, value) in shades.iter_mut() {
                if !used.contains(name) {
                    continue;
                }
                let Some(original) =
                    resolve(self.doc, value, self.limits.max_nesting_depth).cloned()
                else {
                    self.warnings.push(UnconvertedColor {
                        operator: "sh".into(),
                        reason: "unreadable shading resource",
                    });
                    continue;
                };
                let id = self.doc.add_object(original);
                match rewrite_shading(self.doc, id, resources, self.mapper, self.limits) {
                    Ok(()) => *value = Object::Reference(id),
                    Err(reason) => self.warnings.push(UnconvertedColor {
                        operator: "sh".into(),
                        reason,
                    }),
                }
            }
            resources.set("Shading", Object::Dictionary(shades));
        } else if content.operations.iter().any(|op| op.operator == "sh") {
            self.warnings.push(UnconvertedColor {
                operator: "sh".into(),
                reason: "missing shading resource",
            });
        }
        Ok(())
    }
    fn appearances(
        &mut self,
        page: &mut Dictionary,
        resources: &Dictionary,
    ) -> Result<(), PdfError> {
        let Ok(annotation_array) = page.get(b"Annots") else {
            return Ok(());
        };
        let Some(annots) = resolve(self.doc, annotation_array, self.limits.max_nesting_depth)
            .and_then(|o| o.as_array().ok())
            .cloned()
        else {
            self.warnings.push(UnconvertedColor {
                operator: "AP".into(),
                reason: "unreadable annotations",
            });
            return Ok(());
        };
        let mut new_annots = Vec::with_capacity(annots.len());
        for annotation in annots {
            let Some(mut annotation_dict) =
                dict(self.doc, &annotation, self.limits.max_nesting_depth).cloned()
            else {
                self.warnings.push(UnconvertedColor {
                    operator: "AP".into(),
                    reason: "unreadable annotation",
                });
                new_annots.push(annotation);
                continue;
            };
            if let Some(mut ap) = annotation_dict
                .get(b"AP")
                .ok()
                .and_then(|o| dict(self.doc, o, self.limits.max_nesting_depth))
                .cloned()
            {
                for key in [b"N".as_slice(), b"R", b"D"] {
                    let Ok(value) = ap.get(key).cloned() else {
                        continue;
                    };
                    if let Ok(id) = value.as_reference()
                        && self
                            .doc
                            .get_object(id)
                            .is_ok_and(|o| matches!(o, Object::Stream(_)))
                    {
                        ap.set(key, self.enter(id, resources, 1, default_state())?);
                        continue;
                    }
                    if let Some(mut states) =
                        dict(self.doc, &value, self.limits.max_nesting_depth).cloned()
                    {
                        for (_, value) in states.iter_mut() {
                            if let Ok(id) = value.as_reference() {
                                *value = Object::Reference(self.enter(
                                    id,
                                    resources,
                                    1,
                                    default_state(),
                                )?);
                            } else {
                                self.warnings.push(UnconvertedColor {
                                    operator: "AP".into(),
                                    reason: "unreadable appearance state",
                                });
                            }
                        }
                        ap.set(key, states);
                    } else {
                        self.warnings.push(UnconvertedColor {
                            operator: "AP".into(),
                            reason: "unreadable appearance",
                        });
                    }
                }
                annotation_dict.set("AP", ap);
                new_annots.push(Object::Reference(self.doc.add_object(annotation_dict)));
            } else {
                if annotation_dict.get(b"AP").is_ok() {
                    self.warnings.push(UnconvertedColor {
                        operator: "AP".into(),
                        reason: "unreadable appearance dictionary",
                    });
                }
                new_annots.push(annotation)
            }
        }
        page.set("Annots", new_annots);
        Ok(())
    }
}

/// Rewrite page contents and cloned nested resource streams transactionally.
pub fn rewrite_document<F>(
    pdf: &mut PdfDoc,
    mapper: &F,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<UnconvertedColor>, PdfError>
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    let mut working = pdf.clone();
    let budget = working.decode_budget();
    let doc = working.inner_mut()?;
    let pages = super::page_ids(doc, limits, ctx)?;
    let mut walker = Rewrite {
        doc,
        mapper,
        limits,
        ctx,
        budget,
        active: HashSet::new(),
        visits: 0,
        warnings: Vec::new(),
    };
    for page_id in pages {
        ctx.check()?;
        let mut resources = super::page_resources(walker.doc, page_id, limits)?;
        let original_resources = resources.clone();
        let mut bytes = Vec::new();
        for id in walker.doc.get_page_contents(page_id) {
            let stream = walker
                .doc
                .get_object(id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?
                .clone();
            bytes.extend_from_slice(&walker.bytes(&stream)?);
            bytes.push(b'\n');
        }
        let mut content = Content::decode(&bytes).map_err(|_| PdfError::Malformed)?;
        walker.resources(&mut resources, 0, &mut content, default_state())?;
        let mut context = ColorContext {
            limits,
            ctx,
            budget: &mut walker.budget,
        };
        let warnings = rewrite_content_with_state(
            walker.doc,
            &mut content,
            &mut resources,
            mapper,
            &mut context,
            default_state(),
        )?;
        walker.warnings.extend(
            warnings
                .into_iter()
                .filter(|warning| warning.operator != "sh"),
        );
        let output = content.encode().map_err(|_| PdfError::Malformed)?;
        walker.budget.charge(output.len())?;
        let content_id = walker
            .doc
            .add_object(Stream::new(Dictionary::new(), output));
        let mut page = walker
            .doc
            .get_object(page_id)
            .and_then(Object::as_dict)
            .map_err(|_| PdfError::Malformed)?
            .clone();
        walker.appearances(&mut page, &original_resources)?;
        page.set("Resources", resources);
        page.set("Contents", content_id);
        *walker
            .doc
            .get_object_mut(page_id)
            .map_err(|_| PdfError::Malformed)? = Object::Dictionary(page);
    }
    let warnings = std::mem::take(&mut walker.warnings);
    *pdf = working;
    Ok(warnings)
}
