//! Flatten selected form widgets through the shared appearance compositor.

use std::collections::{HashMap, HashSet};

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::form_model::{FieldInfo, FieldType, FormValue},
};
use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    flatten::{self, FlattenFilter},
    forms::{
        EditReport,
        appearance::{self, TextAppearance},
        fill, list, resolve,
    },
    tools::{rotate::collect_page_ids, sanitize::prune_fields},
};

pub(crate) fn unique_marker(
    doc: &Document,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<String, PdfError> {
    if doc.objects.len() > limits.max_blocks as usize {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: limits.max_blocks,
        }));
    }
    let mut stack: Vec<(&Object, usize)> = doc.objects.values().map(|o| (o, 0)).collect();
    let mut names = HashSet::new();
    let mut count = 0u32;
    while let Some((object, depth)) = stack.pop() {
        count = count.saturating_add(1);
        if count > limits.max_blocks {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: limits.max_blocks,
            }));
        }
        if depth > usize::from(limits.max_nesting_depth) {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: limits.max_nesting_depth,
            }));
        }
        if count % 256 == 0 {
            ctx.check()?;
        }
        match object {
            Object::Array(items) => {
                if stack.len().saturating_add(items.len()) > limits.max_blocks as usize {
                    return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                        limit: limits.max_blocks,
                    }));
                }
                stack.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Object::Dictionary(dict) => {
                if stack.len().saturating_add(dict.len()) > limits.max_blocks as usize {
                    return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                        limit: limits.max_blocks,
                    }));
                }
                for (key, value) in dict.iter() {
                    names.insert(key.clone());
                    stack.push((value, depth + 1));
                }
            }
            Object::Stream(stream) => {
                if stack.len().saturating_add(stream.dict.len()) > limits.max_blocks as usize {
                    return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                        limit: limits.max_blocks,
                    }));
                }
                for (key, value) in stream.dict.iter() {
                    names.insert(key.clone());
                    stack.push((value, depth + 1));
                }
            }
            _ => {}
        }
    }
    for n in 0..=limits.max_blocks {
        let marker = format!("AriadFlattenSelect{n}");
        if !names.contains(marker.as_bytes()) {
            return Ok(marker);
        }
    }
    Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
        limit: limits.max_blocks,
    }))
}
fn has_appearance(doc: &Document, id: ObjectId, limits: &Limits) -> bool {
    doc.get_dictionary(id)
        .ok()
        .and_then(|d| d.get(b"AP").ok())
        .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
        .and_then(|v| v.as_dict().ok())
        .and_then(|ap| ap.get(b"N").ok())
        .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
        .is_some_and(|normal| matches!(normal, Object::Stream(_) | Object::Dictionary(_)))
}
fn regenerate(
    doc: &mut Document,
    id: ObjectId,
    info: &FieldInfo,
    reference: &fill::FieldRef,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    if !matches!(
        info.field_type,
        Some(
            FieldType::Text
                | FieldType::Combo
                | FieldType::List
                | FieldType::Checkbox
                | FieldType::Radio
        )
    ) {
        return Ok(false);
    }
    let (width, height) = fill::widget_rect(doc, id, limits.max_nesting_depth)?;
    if matches!(
        info.field_type,
        Some(FieldType::Checkbox | FieldType::Radio)
    ) {
        let state = doc
            .get_dictionary(id)
            .ok()
            .and_then(|dict| dict.get(b"AS").ok())
            .and_then(|value| resolve(doc, value, limits.max_nesting_depth).ok())
            .and_then(|value| value.as_name().ok());
        let checked = match (info.field_type, &info.value, state) {
            (Some(FieldType::Checkbox), FormValue::Bool(value), _) => *value,
            (Some(FieldType::Radio), FormValue::Text(value), Some(state)) => {
                state != b"Off" && state == value.as_bytes()
            }
            (_, _, Some(state)) => state != b"Off",
            _ => false,
        };
        let normal = appearance::button(
            doc,
            width,
            height,
            checked,
            info.field_type == Some(FieldType::Radio),
            ctx,
        )?;
        doc.get_dictionary_mut(id)
            .map_err(|_| PdfError::Malformed)?
            .set("AP", dictionary! { "N" => Object::Reference(normal) });
        return Ok(true);
    }
    if !matches!(
        info.field_type,
        Some(FieldType::Text | FieldType::Combo | FieldType::List)
    ) {
        return Ok(false);
    }
    let (mut value, selected_rows) = fill::display_value(info, &info.value);
    if info.flags.password {
        value = "*".repeat(value.chars().count());
    }
    let style = TextAppearance {
        width,
        height,
        font_size: reference.font_size,
        multiline: info.flags.multiline || info.field_type == Some(FieldType::List),
        wrap_lines: info.field_type != Some(FieldType::List),
        alignment: reference.align,
        comb_cells: if info.flags.comb {
            info.max_length.map(|n| n as usize)
        } else {
            None
        },
        selected_rows,
        paint: appearance::widget_paint(
            doc,
            id,
            &reference.default_appearance,
            reference.standard_font.as_ref(),
            limits.max_nesting_depth,
        ),
    };
    let ap = appearance::text(doc, &value, &style, ctx)?;
    doc.get_dictionary_mut(id)
        .map_err(|_| PdfError::Malformed)?
        .set("AP", dictionary! { "N" => Object::Reference(ap) });
    Ok(true)
}

/// Flatten selected field widgets, retaining hidden or undrawable widgets.
pub fn flatten(
    pdf: &mut PdfDoc,
    pages: &[u32],
    names: Option<&[String]>,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<EditReport, PdfError> {
    ctx.check()?;
    let mut edited = pdf.clone();
    let all_pages = collect_page_ids(edited.inner(), ctx)?;
    let selected: HashSet<u32> = pages.iter().copied().collect();
    if selected
        .iter()
        .any(|page| *page == 0 || *page as usize > all_pages.len())
    {
        return Err(PdfError::InvalidParams(
            "page is outside the document".into(),
        ));
    }
    let chosen_pages: Vec<ObjectId> = all_pages
        .iter()
        .enumerate()
        .filter_map(|(index, id)| selected.contains(&((index + 1) as u32)).then_some(*id))
        .collect();
    let listing = list::list(&edited, pages, true, limits, ctx)?;
    let (refs, _, duplicate_names) = fill::locate(edited.inner(), limits, ctx)?;
    let requested = names.map(|names| names.iter().cloned().collect::<HashSet<_>>());
    let mut infos = HashMap::new();
    for field in &listing.fields {
        if requested
            .as_ref()
            .is_none_or(|set| set.contains(&field.name))
        {
            infos.entry(field.name.clone()).or_insert(field);
        }
    }
    let mut report = EditReport {
        warnings: listing.warnings.clone(),
        ..EditReport::default()
    };
    if duplicate_names {
        report
            .warnings
            .push("duplicate_field_name: only the first field with each name is edited".into());
    }
    if let Some(names) = requested.as_ref() {
        if names.iter().any(|name| !infos.contains_key(name)) {
            report
                .warnings
                .push("unknown_field: selected field was not found on the selected pages".into());
        }
    }
    let mut page_widgets = HashSet::new();
    for &page in &chosen_pages {
        ctx.check()?;
        let d = edited
            .inner()
            .get_dictionary(page)
            .map_err(|_| PdfError::Malformed)?;
        if let Some(annots) = d
            .get(b"Annots")
            .ok()
            .and_then(|v| resolve(edited.inner(), v, limits.max_nesting_depth).ok())
            .and_then(|v| v.as_array().ok())
        {
            for (index, annotation) in annots.iter().enumerate() {
                if index % 256 == 0 {
                    ctx.check()?;
                }
                if let Ok(id) = annotation.as_reference() {
                    page_widgets.insert(id);
                }
            }
        }
    }
    let mut target = HashSet::new();
    for name in infos.keys() {
        if let Some(reference) = refs.get(name) {
            for id in &reference.widgets {
                if page_widgets.contains(id) {
                    target.insert(*id);
                }
            }
        }
    }
    let marker = unique_marker(edited.inner(), limits, ctx)?;
    {
        let doc = edited.inner_mut()?;
        for (name, info) in &infos {
            let Some(reference) = refs.get(name) else {
                continue;
            };
            for &id in &reference.widgets {
                if !target.contains(&id) {
                    continue;
                }
                ctx.check()?;
                let visibility = doc
                    .get_dictionary(id)
                    .ok()
                    .and_then(|d| d.get(b"F").ok())
                    .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
                    .and_then(|v| v.as_i64().ok())
                    .unwrap_or(0);
                if visibility & (2 | 32) != 0 {
                    continue;
                }
                if !has_appearance(doc, id, limits) {
                    match regenerate(doc, id, info, reference, limits, ctx) {
                        Ok(_) => {}
                        Err(PdfError::InvalidParams(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
                doc.get_dictionary_mut(id)
                    .map_err(|_| PdfError::Malformed)?
                    .set(marker.as_bytes(), true);
            }
        }
        let flattened = flatten::flatten_appearances(
            doc,
            &chosen_pages,
            FlattenFilter {
                select: |d: &Dictionary| {
                    d.get(marker.as_bytes())
                        .and_then(Object::as_bool)
                        .unwrap_or(false)
                        && d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget")
                },
                limits,
            },
            ctx,
        )?;
        for id in &target {
            if let Ok(d) = doc.get_dictionary_mut(*id) {
                d.remove(marker.as_bytes());
            }
        }
        prune_fields(
            doc,
            &flattened.flattened,
            &flattened.flattened_direct,
            limits,
            ctx,
        )?;
        if flattened.without_appearance > 0 {
            report
                .warnings
                .push("appearance_unavailable: some widgets were retained".into());
        }
        let done: HashSet<_> = flattened.flattened.iter().copied().collect();
        for (name, info) in &infos {
            if let Some(reference) = refs.get(name) {
                if reference.widgets.iter().any(|id| done.contains(id)) {
                    report.affected.push(name.clone());
                }
                if info.actions.has_javascript
                    && reference.widgets.iter().all(|id| done.contains(id))
                {
                    report
                        .warnings
                        .push("javascript_removed: flattened field actions were removed".into());
                }
            }
        }
    }
    report.affected.sort();
    *pdf = edited;
    Ok(report)
}
