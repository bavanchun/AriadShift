//! Fill existing AcroForm fields without executing their actions.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::form_model::{
        FieldInfo, FieldType, FormValue, MAX_FIELD_DEPTH, MAX_FIELDS, MAX_FILL_VALUE_BYTES,
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    forms::{
        EditReport,
        appearance::{self, TextAppearance},
        list,
    },
    tools::{
        metadata::catalog_object_id, outline::decode_pdf_object_string, rotate::collect_page_ids,
    },
};

/// Whether unknown fields and unsupported values are errors.
#[derive(Clone, Copy, Debug, Default)]
pub struct FillOptions {
    pub strict: bool,
}

#[derive(Clone)]
pub(super) struct FieldRef {
    pub id: ObjectId,
    pub widgets: Vec<ObjectId>,
    pub align: u8,
    pub font_size: f64,
    pub default_appearance: String,
    pub standard_font: Option<Object>,
    pub resources: Option<Arc<Object>>,
    pub has_appearance_font: bool,
}
use super::resolve;

fn get<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8], limit: u16) -> Option<&'a Object> {
    dict.get(key).ok().and_then(|v| resolve(doc, v, limit).ok())
}
fn decode(doc: &Document, value: &Object, limit: u16) -> Option<String> {
    let object = resolve(doc, value, limit).ok()?;
    if matches!(object, Object::String(bytes, _) | Object::Name(bytes) if bytes.len() > MAX_FILL_VALUE_BYTES)
    {
        return None;
    }
    decode_pdf_object_string(object)
}
fn parse_font_size(da: &str) -> Option<f64> {
    let mut previous = "";
    let mut result = None;
    for token in da.split_whitespace().take(256) {
        if token == "Tf" {
            if let Ok(size) = previous.parse::<f64>() {
                if size.is_finite() && (0.0..=1000.0).contains(&size) {
                    result = Some(size);
                }
            }
        }
        previous = token;
    }
    result
}

struct Locator<'a> {
    doc: &'a Document,
    limits: &'a Limits,
    ctx: &'a ToolCtx,
    seen: HashSet<ObjectId>,
    count: u32,
    fields: HashMap<String, FieldRef>,
    duplicate_names: bool,
    resources: HashMap<*const Object, Arc<Object>>,
}
impl Locator<'_> {
    fn visit(
        &mut self,
        obj: &Object,
        prefix: &str,
        align: u8,
        inherited_size: f64,
        inherited_da: &str,
        inherited_resources: Option<&Object>,
        depth: usize,
    ) -> Result<(), PdfError> {
        self.ctx.check()?;
        self.count = self.count.saturating_add(1);
        if self.count > self.limits.max_blocks || self.count as usize > MAX_FIELDS {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: self.limits.max_blocks.min(MAX_FIELDS as u32),
            }));
        }
        if depth > MAX_FIELD_DEPTH || depth > usize::from(self.limits.max_nesting_depth) {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth.min(MAX_FIELD_DEPTH as u16),
            }));
        }
        let id = obj
            .as_reference()
            .map_err(|_| PdfError::InvalidParams("cannot fill a direct field dictionary".into()))?;
        if !self.seen.insert(id) {
            return Ok(());
        }
        let doc = self.doc;
        let dict = doc.get_dictionary(id).map_err(|_| PdfError::Malformed)?;
        let part = dict
            .get(b"T")
            .ok()
            .and_then(|v| decode(self.doc, v, self.limits.max_nesting_depth))
            .unwrap_or_default();
        let name = if prefix.is_empty() {
            part
        } else if part.is_empty() {
            prefix.to_owned()
        } else {
            format!("{prefix}.{part}")
        };
        let align = get(self.doc, dict, b"Q", self.limits.max_nesting_depth)
            .and_then(|o| o.as_i64().ok())
            .and_then(|v| u8::try_from(v).ok())
            .filter(|v| *v <= 2)
            .unwrap_or(align);
        let default_appearance = dict
            .get(b"DA")
            .ok()
            .and_then(|v| decode(self.doc, v, self.limits.max_nesting_depth))
            .unwrap_or_else(|| inherited_da.to_owned());
        let font_size = parse_font_size(&default_appearance).unwrap_or(inherited_size);
        let resources = dict.get(b"DR").ok().or(inherited_resources);
        let kid_array = get(self.doc, dict, b"Kids", self.limits.max_nesting_depth)
            .and_then(|o| o.as_array().ok());
        if kid_array.is_some_and(|kids| {
            kids.len() > MAX_FIELDS || kids.len() > self.limits.max_blocks as usize
        }) {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: self.limits.max_blocks.min(MAX_FIELDS as u32),
            }));
        }
        let kids = kid_array.cloned().unwrap_or_default();
        let mut widgets = Vec::new();
        let mut fields = Vec::new();
        if get(self.doc, dict, b"Subtype", self.limits.max_nesting_depth)
            .and_then(|v| v.as_name().ok())
            == Some(b"Widget")
        {
            widgets.push(id);
        }
        for (index, child) in kids.into_iter().enumerate() {
            if index % 256 == 0 {
                self.ctx.check()?;
            }
            let child_dict = resolve(self.doc, &child, self.limits.max_nesting_depth)?
                .as_dict()
                .map_err(|_| PdfError::Malformed)?;
            if !child_dict.has(b"T")
                && get(
                    self.doc,
                    child_dict,
                    b"Subtype",
                    self.limits.max_nesting_depth,
                )
                .and_then(|v| v.as_name().ok())
                    == Some(b"Widget")
            {
                widgets.push(
                    child.as_reference().map_err(|_| {
                        PdfError::InvalidParams("cannot fill a direct widget".into())
                    })?,
                );
            } else {
                fields.push(child);
            }
        }
        if !name.is_empty() && (fields.is_empty() || !widgets.is_empty()) {
            let has_appearance_font = appearance::font_exists(
                self.doc,
                &default_appearance,
                resources,
                self.limits.max_nesting_depth,
            );
            let shared_resources = if has_appearance_font {
                None
            } else {
                resources.map(|resource| {
                    self.resources
                        .entry(std::ptr::from_ref(resource))
                        .or_insert_with(|| Arc::new(resource.clone()))
                        .clone()
                })
            };
            if let std::collections::hash_map::Entry::Vacant(entry) =
                self.fields.entry(name.clone())
            {
                entry.insert(FieldRef {
                    id,
                    widgets,
                    align,
                    font_size,
                    default_appearance: default_appearance.clone(),
                    standard_font: appearance::standard_font(
                        self.doc,
                        &default_appearance,
                        resources,
                        self.limits.max_nesting_depth,
                    ),
                    resources: shared_resources,
                    has_appearance_font,
                });
            } else {
                self.duplicate_names = true;
            }
        }
        for child in fields {
            self.visit(
                &child,
                &name,
                align,
                font_size,
                &default_appearance,
                resources,
                depth + 1,
            )?;
        }
        Ok(())
    }
}
pub(super) fn locate(
    doc: &Document,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(HashMap<String, FieldRef>, Option<Vec<ObjectId>>, bool), PdfError> {
    let root = catalog_object_id(doc)?;
    let catalog = doc.get_dictionary(root).map_err(|_| PdfError::Malformed)?;
    let form = catalog
        .get(b"AcroForm")
        .ok()
        .and_then(|o| resolve(doc, o, limits.max_nesting_depth).ok())
        .and_then(|o| o.as_dict().ok());
    let mut locator = Locator {
        doc,
        limits,
        ctx,
        seen: HashSet::new(),
        count: 0,
        fields: HashMap::new(),
        duplicate_names: false,
        resources: HashMap::new(),
    };
    let mut calculations = None;
    if let Some(form) = form {
        if let Ok(order) = form.get(b"CO") {
            let order = resolve(doc, order, limits.max_nesting_depth)
                .ok()
                .and_then(|value| value.as_array().ok());
            if order.is_some_and(|items| {
                items.len() > limits.max_blocks as usize || items.len() > MAX_FIELDS
            }) {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: limits.max_blocks.min(MAX_FIELDS as u32),
                }));
            }
            calculations = Some(
                order
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.as_reference().ok())
                    .collect(),
            );
        }
        if let Some(fields) =
            get(doc, form, b"Fields", limits.max_nesting_depth).and_then(|v| v.as_array().ok())
        {
            for field in fields {
                let da = form
                    .get(b"DA")
                    .ok()
                    .and_then(|v| decode(doc, v, limits.max_nesting_depth))
                    .unwrap_or_default();
                let size = parse_font_size(&da).unwrap_or(0.0);
                locator.visit(field, "", 0, size, &da, form.get(b"DR").ok(), 0)?;
            }
        }
    }
    Ok((locator.fields, calculations, locator.duplicate_names))
}

pub(super) fn widget_rect(
    doc: &Document,
    id: ObjectId,
    limit: u16,
) -> Result<(f64, f64), PdfError> {
    let dict = doc.get_dictionary(id).map_err(|_| PdfError::Malformed)?;
    let rect = get(doc, dict, b"Rect", limit)
        .and_then(|v| v.as_array().ok())
        .ok_or(PdfError::Malformed)?;
    if rect.len() != 4 {
        return Err(PdfError::Malformed);
    }
    let mut values = [0.0; 4];
    for (n, object) in rect.iter().enumerate() {
        values[n] = match resolve(doc, object, limit)? {
            Object::Real(v) => f64::from(*v),
            Object::Integer(v) => *v as f64,
            _ => return Err(PdfError::Malformed),
        };
        if !values[n].is_finite() {
            return Err(PdfError::Malformed);
        }
    }
    let (w, h) = (values[2] - values[0], values[3] - values[1]);
    if w <= 0.0 || h <= 0.0 {
        return Err(PdfError::Malformed);
    }
    Ok((w, h))
}
pub(super) fn on_state(doc: &Document, widget: ObjectId, limit: u16) -> Option<String> {
    let dict = doc.get_dictionary(widget).ok()?;
    get(doc, dict, b"AP", limit)?
        .as_dict()
        .ok()
        .and_then(|ap| get(doc, ap, b"N", limit))?
        .as_dict()
        .ok()?
        .iter()
        .find(|(name, _)| name.as_slice() != b"Off")
        .and_then(|(name, _)| String::from_utf8(name.clone()).ok())
}
fn encoded(value: &FormValue) -> Object {
    match value {
        FormValue::Text(s) => crate::pdf_text::encode(s),
        FormValue::Bool(b) => Object::Boolean(*b),
        FormValue::Multi(items) => {
            Object::Array(items.iter().map(|s| crate::pdf_text::encode(s)).collect())
        }
        FormValue::Null(()) => Object::Null,
    }
}
fn install_fallback_font(
    doc: &mut Document,
    field: &FieldRef,
    limit: u16,
    form_installed: &mut bool,
    resource_cache: &mut HashMap<*const Object, ObjectId>,
) -> Result<(), PdfError> {
    if !*form_installed {
        let catalog_id = catalog_object_id(doc)?;
        let form_object = doc
            .get_dictionary(catalog_id)
            .map_err(|_| PdfError::Malformed)?
            .get(b"AcroForm")
            .map_err(|_| PdfError::Malformed)?
            .clone();
        let mut form = resolve(doc, &form_object, limit)?
            .as_dict()
            .map_err(|_| PdfError::Malformed)?
            .clone();
        super::create::add_default_resources(doc, &mut form, limit)?;
        if let Object::Reference(id) = form_object {
            *doc.get_object_mut(id).map_err(|_| PdfError::Malformed)? = Object::Dictionary(form);
        } else {
            doc.get_dictionary_mut(catalog_id)
                .map_err(|_| PdfError::Malformed)?
                .set("AcroForm", form);
        }
        *form_installed = true;
    }
    let local_resource = if let Some(resources) = &field.resources {
        let key = Arc::as_ptr(resources);
        if let Some(id) = resource_cache.get(&key) {
            Some(*id)
        } else {
            let mut local = lopdf::Dictionary::new();
            local.set("DR", resources.as_ref().clone());
            super::create::add_default_resources(doc, &mut local, limit)?;
            let value = local.get(b"DR").map_err(|_| PdfError::Malformed)?.clone();
            let id = doc.add_object(value);
            resource_cache.insert(key, id);
            Some(id)
        }
    } else {
        None
    };
    let mut tokens: Vec<_> = field
        .default_appearance
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if let Some(index) = tokens
        .windows(3)
        .rposition(|parts| parts[2] == "Tf" && parts[0].starts_with('/'))
    {
        tokens[index] = "/Helv".into();
    } else {
        tokens.extend(["/Helv".into(), field.font_size.to_string(), "Tf".into()]);
    }
    let dictionary = doc
        .get_dictionary_mut(field.id)
        .map_err(|_| PdfError::Malformed)?;
    dictionary.set("DA", Object::string_literal(tokens.join(" ")));
    if let Some(id) = local_resource {
        dictionary.set("DR", Object::Reference(id));
    }
    Ok(())
}

fn value_len(value: &FormValue) -> usize {
    match value {
        FormValue::Text(s) => s.len(),
        FormValue::Multi(v) => v.iter().fold(0usize, |n, s| n.saturating_add(s.len())),
        _ => 0,
    }
}
fn prepare(
    info: &FieldInfo,
    value: &FormValue,
    warnings: &mut Vec<String>,
) -> Result<FormValue, PdfError> {
    if value_len(value) > MAX_FILL_VALUE_BYTES {
        return Err(PdfError::InvalidParams("form value is too large".into()));
    }
    if matches!(value, FormValue::Null(())) {
        return Ok(value.clone());
    }
    match info.field_type {
        Some(FieldType::Text) => {
            let FormValue::Text(text) = value else {
                return Err(PdfError::InvalidParams(
                    "text field requires a string".into(),
                ));
            };
            let mut text = text.clone();
            if text.contains('\t') {
                warnings.push("tabs_expanded: tabs are displayed as spaces".into());
            }
            if !info.flags.multiline && text.contains(['\r', '\n']) {
                text = text.replace(['\r', '\n'], " ");
                warnings.push("newlines_replaced: single-line field contained newlines".into());
            }
            if let Some(max) = info.max_length {
                if text.chars().count() > max as usize {
                    text = text.chars().take(max as usize).collect();
                    warnings
                        .push("value_truncated: field value exceeded its maximum length".into());
                }
            }
            Ok(FormValue::Text(text))
        }
        Some(FieldType::Checkbox) => {
            if matches!(value, FormValue::Bool(_) | FormValue::Text(_)) {
                Ok(value.clone())
            } else {
                Err(PdfError::InvalidParams("invalid checkbox value".into()))
            }
        }
        Some(FieldType::Radio) => {
            if matches!(value, FormValue::Text(_)) {
                Ok(value.clone())
            } else {
                Err(PdfError::InvalidParams(
                    "radio field requires a state name".into(),
                ))
            }
        }
        Some(FieldType::Combo | FieldType::List) => {
            let requested: Vec<&str> = match value {
                FormValue::Text(s) => vec![s],
                FormValue::Multi(v) if info.flags.multi_select => {
                    v.iter().map(String::as_str).collect()
                }
                _ => return Err(PdfError::InvalidParams("invalid choice value".into())),
            };
            if !(info.field_type == Some(FieldType::Combo) && info.flags.editable)
                && requested
                    .iter()
                    .any(|item| !item.is_empty() && !info.options.iter().any(|o| o.export == *item))
            {
                return Err(PdfError::InvalidParams(
                    "choice is not an available option".into(),
                ));
            }
            Ok(value.clone())
        }
        _ => Err(PdfError::InvalidParams("field is not fillable".into())),
    }
}

/// Resolve choice export values to display labels and selected row indices.
pub(super) fn display_value(info: &FieldInfo, value: &FormValue) -> (String, Vec<usize>) {
    let selected: Vec<&str> = match value {
        FormValue::Text(value) => vec![value],
        FormValue::Multi(values) => values.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    };
    let indices: Vec<usize> = info
        .options
        .iter()
        .enumerate()
        .filter_map(|(index, option)| selected.contains(&option.export.as_str()).then_some(index))
        .collect();
    match info.field_type {
        Some(FieldType::List) => (
            info.options
                .iter()
                .map(|option| option.display.replace(['\r', '\n'], " "))
                .collect::<Vec<_>>()
                .join("\n"),
            indices,
        ),
        Some(FieldType::Combo) => (
            indices
                .first()
                .and_then(|index| info.options.get(*index))
                .map_or_else(
                    || selected.first().copied().unwrap_or_default().to_owned(),
                    |option| option.display.clone(),
                ),
            Vec::new(),
        ),
        _ => (
            match value {
                FormValue::Text(text) => text.clone(),
                FormValue::Multi(values) => values.join("\n"),
                _ => String::new(),
            },
            Vec::new(),
        ),
    }
}

/// Apply values and regenerate every changed widget's appearance atomically.
pub fn fill(
    pdf: &mut PdfDoc,
    values: &BTreeMap<String, FormValue>,
    options: &FillOptions,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<EditReport, PdfError> {
    ctx.check()?;
    if values.len() > MAX_FIELDS {
        return Err(PdfError::InvalidParams("too many form values".into()));
    }
    let mut edited = pdf.clone();
    let ids = collect_page_ids(edited.inner(), ctx)?;
    let pages: Vec<u32> = (1..=ids.len() as u32).collect();
    let listing = list::list(&edited, &pages, true, limits, ctx)?;
    let mut report = EditReport {
        warnings: listing.warnings.clone(),
        ..EditReport::default()
    };
    let mut infos = HashMap::new();
    for info in listing.fields {
        infos.entry(info.name.clone()).or_insert(info);
    }
    let (refs, has_calculations, duplicate_names) = locate(edited.inner(), limits, ctx)?;
    if duplicate_names {
        report
            .warnings
            .push("duplicate_field_name: only the first field with each name is edited".into());
    }
    let order: HashSet<_> = has_calculations
        .as_ref()
        .into_iter()
        .flatten()
        .copied()
        .collect();
    let mut calculation_names: Vec<_> = infos
        .iter()
        .filter_map(|(name, info)| {
            let has_action = info.actions.kinds.iter().any(|kind| kind == "calculate");
            let is_ordered = refs
                .get(name)
                .is_some_and(|field| order.contains(&field.id));
            (has_action || is_ordered).then_some(name.as_str())
        })
        .collect();
    if has_calculations.is_some() || !calculation_names.is_empty() {
        calculation_names.sort_unstable();
        calculation_names.truncate(10);
        let detail = if calculation_names.is_empty() {
            String::new()
        } else {
            format!(" for {}", calculation_names.join(", "))
        };
        report.warnings.push(format!(
            "calculation_not_run: form calculations were not executed{detail}"
        ));
    }
    let mut form_font_installed = false;
    let mut fallback_resources = HashMap::new();
    for (name, input) in values {
        ctx.check()?;
        let Some(info) = infos.get(name) else {
            if options.strict {
                return Err(PdfError::InvalidParams("unknown field".into()));
            }
            report
                .warnings
                .push("unknown_field: field was not found".into());
            continue;
        };
        if info.flags.read_only {
            report
                .warnings
                .push("field_read_only: field was left unchanged".into());
            continue;
        }
        if matches!(
            info.field_type,
            None | Some(FieldType::Button | FieldType::Signature)
        ) {
            report
                .warnings
                .push("field_not_fillable: field was left unchanged".into());
            continue;
        }
        let value = prepare(info, input, &mut report.warnings)?;
        let reference = refs
            .get(name)
            .ok_or_else(|| PdfError::InvalidParams("field cannot be updated".into()))?;
        let doc = edited.inner_mut()?;
        if matches!(
            info.field_type,
            Some(FieldType::Text | FieldType::Combo | FieldType::List)
        ) && !reference.has_appearance_font
        {
            install_fallback_font(
                doc,
                reference,
                limits.max_nesting_depth,
                &mut form_font_installed,
                &mut fallback_resources,
            )?;
        }
        let mut stored = encoded(&value);
        if matches!(
            info.field_type,
            Some(FieldType::Checkbox | FieldType::Radio)
        ) {
            let mut widget_states = Vec::new();
            for (index, &widget) in reference.widgets.iter().enumerate() {
                let state = on_state(doc, widget, limits.max_nesting_depth)
                    .or_else(|| {
                        doc.get_dictionary(widget)
                            .ok()
                            .and_then(|d| d.get(b"AS").ok())
                            .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
                            .and_then(|v| v.as_name().ok())
                            .filter(|state| *state != b"Off")
                            .and_then(|state| String::from_utf8(state.to_vec()).ok())
                    })
                    .or_else(|| info.options.get(index).map(|option| option.export.clone()));
                widget_states.push(state);
            }
            let selected = match &value {
                FormValue::Null(()) | FormValue::Bool(false) => "Off".to_owned(),
                FormValue::Bool(true) if info.field_type == Some(FieldType::Checkbox) => {
                    widget_states
                        .first()
                        .and_then(Clone::clone)
                        .ok_or_else(|| {
                            PdfError::InvalidParams(
                                "button widget has no discoverable on-state".into(),
                            )
                        })?
                }
                FormValue::Text(s) => s.clone(),
                _ => return Err(PdfError::InvalidParams("invalid button value".into())),
            };
            if selected != "Off"
                && !widget_states
                    .iter()
                    .any(|state| state.as_deref() == Some(selected.as_str()))
            {
                return Err(PdfError::InvalidParams("unknown button state".into()));
            }
            stored = Object::Name(selected.as_bytes().to_vec());
            for (&widget, state) in reference.widgets.iter().zip(&widget_states) {
                // Keep existing normal appearance streams: they may carry the author's styling
                // and can be shared with other widgets. Generate only missing state streams.
                let mut ap = doc
                    .get_dictionary(widget)
                    .ok()
                    .and_then(|d| get(doc, d, b"AP", limits.max_nesting_depth))
                    .and_then(|o| o.as_dict().ok())
                    .cloned()
                    .unwrap_or_default();
                let mut normal = ap
                    .get(b"N")
                    .ok()
                    .and_then(|o| resolve(doc, o, limits.max_nesting_depth).ok())
                    .and_then(|o| o.as_dict().ok())
                    .cloned()
                    .unwrap_or_default();
                let (width, height) = widget_rect(doc, widget, limits.max_nesting_depth)?;
                for (name, checked) in
                    std::iter::once(("Off", false)).chain(state.as_deref().map(|name| (name, true)))
                {
                    let usable = normal
                        .get(name.as_bytes())
                        .ok()
                        .and_then(|o| resolve(doc, o, limits.max_nesting_depth).ok())
                        .is_some_and(|o| matches!(o, Object::Stream(_)));
                    if !usable {
                        let stream = appearance::button(
                            doc,
                            width,
                            height,
                            checked,
                            info.field_type == Some(FieldType::Radio),
                            ctx,
                        )?;
                        normal.set(name.as_bytes(), Object::Reference(stream));
                    }
                }
                ap.set("N", normal);
                ap.remove(b"D");
                let dictionary = doc
                    .get_dictionary_mut(widget)
                    .map_err(|_| PdfError::Malformed)?;
                dictionary.set("AP", ap);
                dictionary.set(
                    "AS",
                    Object::Name(
                        if state.as_deref() == Some(selected.as_str()) {
                            selected.as_bytes()
                        } else {
                            b"Off"
                        }
                        .to_vec(),
                    ),
                );
            }
        } else {
            let (mut text, selected_rows) = display_value(info, &value);
            if info.flags.password {
                text = "*".repeat(text.chars().count());
            }
            for &widget in &reference.widgets {
                let (width, height) = widget_rect(doc, widget, limits.max_nesting_depth)?;
                let style = TextAppearance {
                    width,
                    height,
                    font_size: reference.font_size,
                    multiline: info.flags.multiline
                        || matches!(info.field_type, Some(FieldType::List)),
                    wrap_lines: info.field_type != Some(FieldType::List),
                    alignment: reference.align,
                    comb_cells: if info.flags.comb {
                        info.max_length.map(|n| n as usize)
                    } else {
                        None
                    },
                    selected_rows: selected_rows.clone(),
                    paint: appearance::widget_paint(
                        doc,
                        widget,
                        &reference.default_appearance,
                        reference.standard_font.as_ref(),
                        limits.max_nesting_depth,
                    ),
                };
                let normal = appearance::text(doc, &text, &style, ctx)?;
                doc.get_dictionary_mut(widget)
                    .map_err(|_| PdfError::Malformed)?
                    .set("AP", dictionary! { "N" => Object::Reference(normal) });
            }
        }
        let field = doc
            .get_dictionary_mut(reference.id)
            .map_err(|_| PdfError::Malformed)?;
        if matches!(value, FormValue::Null(())) {
            field.remove(b"V");
        } else {
            field.set("V", stored);
        }
        if matches!(info.field_type, Some(FieldType::Combo | FieldType::List)) {
            let selected = display_value(info, &value).1;
            if selected.is_empty() {
                field.remove(b"I");
            } else {
                field.set(
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
        if field.remove(b"RV").is_some() {
            report
                .warnings
                .push("rich_value_removed: stale rich text was removed".into());
        }
        if info.actions.has_javascript {
            report
                .warnings
                .push("javascript_not_executed: form JavaScript was not executed".into());
        }
        report.affected.push(name.clone());
    }
    *pdf = edited;
    Ok(report)
}
