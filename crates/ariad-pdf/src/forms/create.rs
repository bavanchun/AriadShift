//! Creation of AcroForm fields and their normal appearances.

use std::collections::{BTreeMap, HashMap, HashSet};

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::{
        form_model::{FieldType, FormValue, MAX_FIELD_DEPTH, MAX_NEW_FIELDS},
        forms::NewField,
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    forms::{
        EditReport,
        appearance::{self, TextAppearance},
    },
    tools::{
        metadata::catalog_object_id, outline::decode_pdf_object_string, rotate::collect_page_ids,
    },
};

use super::resolve;

fn dictionary(doc: &Document, value: &Object, limit: u16) -> Result<Dictionary, PdfError> {
    resolve(doc, value, limit)?
        .as_dict()
        .cloned()
        .map_err(|_| PdfError::Malformed)
}
fn array(doc: &Document, value: &Object, limit: u16) -> Result<Vec<Object>, PdfError> {
    resolve(doc, value, limit)?
        .as_array()
        .cloned()
        .map_err(|_| PdfError::Malformed)
}
struct WidgetCollection<'a> {
    seen: HashSet<ObjectId>,
    remaining: &'a mut u32,
    out: &'a mut HashSet<ObjectId>,
}

fn collect_widgets(
    doc: &Document,
    value: &Object,
    limit: u16,
    state: &mut WidgetCollection<'_>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    if depth > MAX_FIELD_DEPTH || depth > usize::from(limit) {
        return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
            limit: limit.min(MAX_FIELD_DEPTH as u16),
        }));
    }
    if *state.remaining == 0 {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: 0,
        }));
    }
    *state.remaining -= 1;
    if let Ok(id) = value.as_reference()
        && !state.seen.insert(id)
    {
        return Err(PdfError::Malformed);
    }
    let field = dictionary(doc, value, limit)?;
    if field
        .get(b"Subtype")
        .ok()
        .and_then(|v| resolve(doc, v, limit).ok())
        .and_then(|v| v.as_name().ok())
        == Some(b"Widget")
    {
        if let Ok(id) = value.as_reference() {
            state.out.insert(id);
        } else {
            return Err(PdfError::InvalidParams(
                "cannot replace a direct widget".into(),
            ));
        }
    }
    if let Ok(kids) = field.get(b"Kids") {
        for child in array(doc, kids, limit)? {
            collect_widgets(doc, &child, limit, state, depth + 1, ctx)?;
        }
    }
    Ok(())
}
fn default_value(
    value: Option<&FormValue>,
    button: bool,
    on: Option<&str>,
) -> Result<Option<Object>, PdfError> {
    match value {
        None | Some(FormValue::Null(())) => Ok(None),
        Some(FormValue::Text(s)) if button => Ok(Some(Object::Name(s.as_bytes().to_vec()))),
        Some(FormValue::Text(s)) => Ok(Some(crate::pdf_text::encode(s))),
        Some(FormValue::Bool(b)) if button => Ok(Some(Object::Name(
            if *b {
                on.ok_or_else(|| PdfError::InvalidParams("button requires an export value".into()))?
            } else {
                "Off"
            }
            .as_bytes()
            .to_vec(),
        ))),
        Some(FormValue::Multi(items)) if !button => Ok(Some(Object::Array(
            items.iter().map(|s| crate::pdf_text::encode(s)).collect(),
        ))),
        _ => Err(PdfError::InvalidParams("invalid default form value".into())),
    }
}
fn default_text(field: &NewField) -> (String, Vec<usize>) {
    let options = field.options.as_deref().unwrap_or_default();
    let selected: Vec<&str> = match field.default.as_ref() {
        Some(FormValue::Text(value)) => vec![value],
        Some(FormValue::Multi(values)) => values.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    };
    let indices: Vec<usize> = options
        .iter()
        .enumerate()
        .filter_map(|(index, option)| selected.contains(&option.export.as_str()).then_some(index))
        .collect();
    if field.field_type == FieldType::List {
        return (
            options
                .iter()
                .map(|option| option.display.replace(['\r', '\n'], " "))
                .collect::<Vec<_>>()
                .join("\n"),
            indices,
        );
    }
    if field.field_type == FieldType::Combo {
        return (
            indices
                .first()
                .and_then(|index| options.get(*index))
                .map_or_else(
                    || selected.first().copied().unwrap_or_default().to_owned(),
                    |option| option.display.clone(),
                ),
            Vec::new(),
        );
    }
    let text = match field.default.as_ref() {
        Some(FormValue::Text(s)) => s.clone(),
        Some(FormValue::Multi(v)) => v.join("\n"),
        _ => String::new(),
    };
    (text, Vec::new())
}
pub(super) fn add_default_resources(
    doc: &mut Document,
    form: &mut Dictionary,
    limit: u16,
) -> Result<(), PdfError> {
    let mut resources = form
        .get(b"DR")
        .ok()
        .map(|v| dictionary(doc, v, limit))
        .transpose()?
        .unwrap_or_default();
    let mut fonts = resources
        .get(b"Font")
        .ok()
        .map(|v| dictionary(doc, v, limit))
        .transpose()?
        .unwrap_or_default();
    if !fonts
        .get(b"Helv")
        .ok()
        .and_then(|value| resolve(doc, value, limit).ok())
        .is_some_and(|value| {
            value
                .as_dict()
                .ok()
                .and_then(|font| font.get(b"Subtype").ok())
                .and_then(|subtype| resolve(doc, subtype, limit).ok())
                .and_then(|subtype| subtype.as_name().ok())
                .is_some()
        })
    {
        let id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
        fonts.set("Helv", Object::Reference(id));
    }
    if !fonts
        .get(b"ZaDb")
        .ok()
        .and_then(|value| resolve(doc, value, limit).ok())
        .is_some_and(|value| {
            value
                .as_dict()
                .ok()
                .and_then(|font| font.get(b"Subtype").ok())
                .and_then(|subtype| resolve(doc, subtype, limit).ok())
                .and_then(|subtype| subtype.as_name().ok())
                .is_some()
        })
    {
        let id = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ZapfDingbats" },
        );
        fonts.set("ZaDb", Object::Reference(id));
    }
    resources.set("Font", fonts);
    form.set("DR", resources);
    if !form.has(b"DA") {
        form.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
    }
    Ok(())
}
fn flags(field: &NewField) -> i64 {
    let mut flags = i64::from(field.read_only) | (i64::from(field.required) << 1);
    if field.multiline {
        flags |= 1 << 12;
    }
    if field.field_type == FieldType::List
        && matches!(field.default.as_ref(), Some(FormValue::Multi(_)))
    {
        flags |= 1 << 21;
    }
    match field.field_type {
        FieldType::Radio => flags |= 1 << 15,
        FieldType::Button => flags |= 1 << 16,
        FieldType::Combo => flags |= 1 << 17,
        _ => {}
    }
    flags
}
fn base_field(field: &NewField) -> Result<Dictionary, PdfError> {
    let ft = match field.field_type {
        FieldType::Text => "Tx",
        FieldType::Checkbox | FieldType::Radio | FieldType::Button => "Btn",
        FieldType::Combo | FieldType::List => "Ch",
        FieldType::Signature => "Sig",
    };
    let mut out = dictionary! { "FT" => Object::Name(ft.as_bytes().to_vec()), "T" => crate::pdf_text::encode(&field.name), "Ff" => flags(field) };
    if let Some(tooltip) = &field.tooltip {
        out.set("TU", crate::pdf_text::encode(tooltip));
    }
    if let Some(max) = field.max_length {
        out.set("MaxLen", i64::from(max));
    }
    if matches!(
        field.field_type,
        FieldType::Text | FieldType::Combo | FieldType::List
    ) {
        out.set(
            "DA",
            Object::string_literal(format!("/Helv {:.2} Tf 0 g", field.font_size)),
        );
    }
    if let Some(options) = &field.options {
        out.set(
            "Opt",
            Object::Array(
                options
                    .iter()
                    .map(|o| {
                        Object::Array(vec![
                            crate::pdf_text::encode(&o.export),
                            crate::pdf_text::encode(&o.display),
                        ])
                    })
                    .collect(),
            ),
        );
    }
    if field.field_type == FieldType::List && field.default.is_some() {
        let (_, selected) = default_text(field);
        if !selected.is_empty() {
            out.set(
                "I",
                Object::Array(
                    selected
                        .into_iter()
                        .map(|index| Object::Integer(index as i64))
                        .collect(),
                ),
            );
        }
    }
    if let Some(default) = default_value(
        field.default.as_ref(),
        matches!(field.field_type, FieldType::Checkbox | FieldType::Radio),
        field.export_value.as_deref(),
    )? {
        out.set("V", default.clone());
        out.set("DV", default);
    }
    Ok(out)
}
fn widget(
    doc: &mut Document,
    field: &NewField,
    page: ObjectId,
    parent: Option<ObjectId>,
    standard_font: Option<&Object>,
    ctx: &ToolCtx,
) -> Result<ObjectId, PdfError> {
    ctx.check()?;
    let r = field.rect;
    if r.iter().any(|n| n.abs() > 1_000_000.0) {
        return Err(PdfError::InvalidParams(
            "field coordinates are out of range".into(),
        ));
    }
    let mut dict = if parent.is_some() {
        Dictionary::new()
    } else {
        base_field(field)?
    };
    dict.set("Type", "Annot");
    dict.set("Subtype", "Widget");
    dict.set("F", 4);
    dict.set("P", Object::Reference(page));
    dict.set(
        "Rect",
        Object::Array(r.iter().map(|v| Object::Real(*v as f32)).collect()),
    );
    if let Some(parent) = parent {
        dict.set("Parent", Object::Reference(parent));
    }
    let width = r[2] - r[0];
    let height = r[3] - r[1];
    if matches!(field.field_type, FieldType::Checkbox | FieldType::Radio) {
        let state = field
            .export_value
            .as_deref()
            .ok_or_else(|| PdfError::InvalidParams("missing export value".into()))?;
        let off = appearance::button(
            doc,
            width,
            height,
            false,
            field.field_type == FieldType::Radio,
            ctx,
        )?;
        let on = appearance::button(
            doc,
            width,
            height,
            true,
            field.field_type == FieldType::Radio,
            ctx,
        )?;
        let mut normal = dictionary! { "Off" => Object::Reference(off) };
        normal.set(state.as_bytes(), Object::Reference(on));
        let selected = match field.default.as_ref() {
            Some(FormValue::Bool(true)) => true,
            Some(FormValue::Text(s)) => s == state,
            _ => false,
        };
        dict.set(
            "AS",
            Object::Name(if selected { state } else { "Off" }.as_bytes().to_vec()),
        );
        dict.set("AP", dictionary! { "N" => normal });
        if parent.is_none() && !dict.has(b"V") {
            dict.set("V", "Off");
        }
    } else {
        let (text, selected_rows) = default_text(field);
        let style = TextAppearance {
            width,
            height,
            font_size: field.font_size,
            multiline: field.multiline || field.field_type == FieldType::List,
            wrap_lines: field.field_type != FieldType::List,
            alignment: 0,
            comb_cells: None,
            selected_rows,
            paint: appearance::WidgetPaint {
                standard_font: standard_font.cloned(),
                ..appearance::WidgetPaint::default()
            },
        };
        let normal = appearance::text(doc, &text, &style, ctx)?;
        dict.set("AP", dictionary! { "N" => Object::Reference(normal) });
    }
    Ok(doc.add_object(dict))
}

/// Create fields atomically in a cloned document, preserving unrelated form data.
pub fn create(
    pdf: &mut PdfDoc,
    fields: &[NewField],
    replace_existing: bool,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<EditReport, PdfError> {
    ctx.check()?;
    if fields.is_empty()
        || fields.len() > MAX_NEW_FIELDS
        || fields.len() > limits.max_blocks as usize
    {
        return Err(PdfError::InvalidParams("invalid number of fields".into()));
    }
    for field in fields {
        field.validate().map_err(PdfError::InvalidParams)?;
    }
    let mut grouped: BTreeMap<&str, Vec<&NewField>> = BTreeMap::new();
    for field in fields {
        grouped.entry(&field.name).or_default().push(field);
    }
    for group in grouped.values() {
        if group[0].field_type == FieldType::Radio {
            if group[0]
                .default
                .as_ref()
                .is_some_and(|v| !matches!(v, FormValue::Text(_) | FormValue::Null(())))
            {
                return Err(PdfError::InvalidParams(
                    "radio default must be a state name".into(),
                ));
            }
            if let Some(FormValue::Text(value)) = &group[0].default
                && value != "Off"
                && !group
                    .iter()
                    .any(|f| f.export_value.as_deref() == Some(value.as_str()))
            {
                return Err(PdfError::InvalidParams(
                    "radio default is not an export value".into(),
                ));
            }
        }
        if group.len() > 1 {
            if group.iter().any(|f| f.field_type != FieldType::Radio) {
                return Err(PdfError::InvalidParams("duplicate field name".into()));
            }
            let mut states = HashSet::new();
            if group
                .iter()
                .any(|f| !states.insert(f.export_value.as_deref()))
            {
                return Err(PdfError::InvalidParams(
                    "duplicate radio export value".into(),
                ));
            }
            if group.iter().any(|f| {
                f.default != group[0].default
                    || f.required != group[0].required
                    || f.read_only != group[0].read_only
            }) {
                return Err(PdfError::InvalidParams(
                    "radio group attributes differ".into(),
                ));
            }
        }
    }
    let mut edited = pdf.clone();
    let page_ids = collect_page_ids(edited.inner(), ctx)?;
    for field in fields {
        if field.page as usize > page_ids.len() {
            return Err(PdfError::InvalidParams(
                "field page is outside the document".into(),
            ));
        }
    }
    let doc = edited.inner_mut()?;
    let catalog_id = catalog_object_id(doc)?;
    let catalog = doc
        .get_dictionary(catalog_id)
        .map_err(|_| PdfError::Malformed)?;
    let form_obj = catalog.get(b"AcroForm").ok().cloned();
    let mut form = form_obj
        .as_ref()
        .map(|v| dictionary(doc, v, limits.max_nesting_depth))
        .transpose()?
        .unwrap_or_default();
    let has_xfa = form.has(b"XFA");
    let existing = form
        .get(b"Fields")
        .ok()
        .map(|v| array(doc, v, limits.max_nesting_depth))
        .transpose()?
        .unwrap_or_default();
    let mut retained = Vec::new();
    let mut removed_widgets = HashSet::new();
    let mut remaining = limits.max_blocks;
    for entry in existing {
        ctx.check()?;
        let existing_field = dictionary(doc, &entry, limits.max_nesting_depth)?;
        let name = existing_field
            .get(b"T")
            .ok()
            .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
            .and_then(decode_pdf_object_string);
        if name.as_deref().is_some_and(|n| grouped.contains_key(n)) {
            if !replace_existing {
                return Err(PdfError::InvalidParams("field name already exists".into()));
            }
            collect_widgets(
                doc,
                &entry,
                limits.max_nesting_depth,
                &mut WidgetCollection {
                    seen: HashSet::new(),
                    remaining: &mut remaining,
                    out: &mut removed_widgets,
                },
                0,
                ctx,
            )?;
        } else {
            retained.push(entry);
        }
    }
    add_default_resources(doc, &mut form, limits.max_nesting_depth)?;
    let standard_font = appearance::standard_font(
        doc,
        "/Helv 0 Tf 0 g",
        form.get(b"DR").ok(),
        limits.max_nesting_depth,
    );
    let mut additions: HashMap<ObjectId, Vec<(Option<u32>, usize, ObjectId)>> = HashMap::new();
    for (sequence, group) in grouped.values().enumerate() {
        let first = group[0];
        if first.field_type == FieldType::Radio {
            let mut parent = base_field(first)?;
            if !parent.has(b"V") {
                parent.set("V", "Off");
            }
            let parent_id = doc.add_object(parent);
            let mut kids = Vec::new();
            for field in group {
                let page = page_ids[field.page as usize - 1];
                let id = widget(
                    doc,
                    field,
                    page,
                    Some(parent_id),
                    standard_font.as_ref(),
                    ctx,
                )?;
                kids.push(Object::Reference(id));
                additions
                    .entry(page)
                    .or_default()
                    .push((field.tab_order, sequence, id));
            }
            doc.get_dictionary_mut(parent_id)
                .map_err(|_| PdfError::Malformed)?
                .set("Kids", Object::Array(kids));
            retained.push(Object::Reference(parent_id));
        } else {
            let page = page_ids[first.page as usize - 1];
            let id = widget(doc, first, page, None, standard_font.as_ref(), ctx)?;
            additions
                .entry(page)
                .or_default()
                .push((first.tab_order, sequence, id));
            retained.push(Object::Reference(id));
        }
    }
    for page_id in &page_ids {
        ctx.check()?;
        let page = doc
            .get_dictionary(*page_id)
            .map_err(|_| PdfError::Malformed)?;
        let existing = page
            .get(b"Annots")
            .ok()
            .map(|v| array(doc, v, limits.max_nesting_depth))
            .transpose()?
            .unwrap_or_default();
        let old_count = existing.len();
        let mut annots: Vec<_> = existing
            .into_iter()
            .filter(|v| {
                v.as_reference()
                    .ok()
                    .is_none_or(|id| !removed_widgets.contains(&id))
            })
            .collect();
        let had_removal = annots.len() != old_count;
        let new = additions.remove(page_id);
        let had_addition = new.is_some();
        if let Some(mut new) = new {
            new.sort_by_key(|(order, sequence, _)| (order.unwrap_or(u32::MAX), *sequence));
            annots.extend(new.into_iter().map(|(_, _, id)| Object::Reference(id)));
        }
        if had_removal || had_addition {
            let page = doc
                .get_dictionary_mut(*page_id)
                .map_err(|_| PdfError::Malformed)?;
            if annots.is_empty() {
                page.remove(b"Annots");
            } else {
                page.set("Annots", Object::Array(annots));
            }
            if had_addition {
                page.set("Tabs", "R");
            }
        }
    }
    form.set("Fields", Object::Array(retained));
    if let Some(Object::Reference(id)) = form_obj {
        *doc.get_object_mut(id).map_err(|_| PdfError::Malformed)? = Object::Dictionary(form);
    } else {
        doc.get_dictionary_mut(catalog_id)
            .map_err(|_| PdfError::Malformed)?
            .set("AcroForm", form);
    }
    *pdf = edited;
    Ok(EditReport {
        affected: grouped.keys().map(|s| (*s).to_owned()).collect(),
        warnings: if has_xfa {
            vec!["xfa_ignored: dynamic XFA content is not processed".into()]
        } else {
            Vec::new()
        },
    })
}
