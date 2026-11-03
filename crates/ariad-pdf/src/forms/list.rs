//! Bounded traversal of AcroForm field and widget dictionaries.

use std::collections::{BTreeMap, HashMap, HashSet};

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::form_model::{
        ActionSummary, ChoiceOption, FieldFlags, FieldInfo, FieldType, FormValue, MAX_FIELD_DEPTH,
        MAX_FIELDS, MAX_FILL_VALUE_BYTES, WidgetInfo,
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::Serialize;

use crate::{
    PdfDoc, PdfError, ToolCtx,
    tools::{
        metadata::catalog_object_id, outline::decode_pdf_object_string, rotate::collect_page_ids,
    },
};

/// Result of a static field traversal. No embedded actions are executed.
#[derive(Clone, Debug, Default, Serialize)]
pub struct FormsListing {
    pub fields: Vec<FieldInfo>,
    pub xfa: bool,
    pub need_appearances: bool,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

use super::resolve;

fn get<'a>(doc: &'a Document, d: &'a Dictionary, key: &[u8], depth: u16) -> Option<&'a Object> {
    d.get(key).ok().and_then(|v| resolve(doc, v, depth).ok())
}
fn decoded(doc: &Document, value: &Object, depth: u16) -> Option<String> {
    let object = resolve(doc, value, depth).ok()?;
    if matches!(object, Object::String(bytes, _) | Object::Name(bytes) if bytes.len() > MAX_FILL_VALUE_BYTES)
    {
        return None;
    }
    if let Object::Name(name) = object {
        return String::from_utf8(name.to_vec()).ok();
    }
    decode_pdf_object_string(object)
}
fn rect(doc: &Document, dictionary: &Dictionary, depth: u16) -> Option<[f64; 4]> {
    let values = get(doc, dictionary, b"Rect", depth)?.as_array().ok()?;
    if values.len() != 4 {
        return None;
    }
    let mut out = [0.0; 4];
    for (index, value) in values.iter().enumerate() {
        let value = resolve(doc, value, depth).ok()?;
        out[index] = match value {
            Object::Integer(n) => *n as f64,
            Object::Real(n) => f64::from(*n),
            _ => return None,
        };
        if !out[index].is_finite() {
            return None;
        }
    }
    Some(out)
}
fn form_value(doc: &Document, value: Option<&Object>, depth: u16) -> FormValue {
    let Some(value) = value.and_then(|v| resolve(doc, v, depth).ok()) else {
        return FormValue::Null(());
    };
    if let Object::Array(values) = value {
        return FormValue::Multi(
            values
                .iter()
                .filter_map(|v| decoded(doc, v, depth))
                .collect(),
        );
    }
    decoded(doc, value, depth).map_or(FormValue::Null(()), FormValue::Text)
}
fn field_type(ft: Option<&[u8]>, flags: i64) -> Option<FieldType> {
    match ft {
        Some(b"Tx") => Some(FieldType::Text),
        Some(b"Ch") if flags & (1 << 17) != 0 => Some(FieldType::Combo),
        Some(b"Ch") => Some(FieldType::List),
        Some(b"Btn") if flags & (1 << 16) != 0 => Some(FieldType::Button),
        Some(b"Btn") if flags & (1 << 15) != 0 => Some(FieldType::Radio),
        Some(b"Btn") => Some(FieldType::Checkbox),
        Some(b"Sig") => Some(FieldType::Signature),
        _ => None,
    }
}
fn flags(value: i64) -> FieldFlags {
    FieldFlags {
        read_only: value & 1 != 0,
        required: value & 2 != 0,
        no_export: value & 4 != 0,
        multiline: value & (1 << 12) != 0,
        password: value & (1 << 13) != 0,
        comb: value & (1 << 24) != 0,
        editable: value & (1 << 18) != 0,
        multi_select: value & (1 << 21) != 0,
    }
}
fn action_summary(
    doc: &Document,
    field: &Dictionary,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ActionSummary, PdfError> {
    let depth = limits.max_nesting_depth;
    let mut actions = BTreeMap::new();
    if let Some(a) = get(doc, field, b"A", depth).and_then(|v| v.as_dict().ok()) {
        actions.insert("action".to_owned(), a);
    }
    if let Some(aa) = get(doc, field, b"AA", depth).and_then(|v| v.as_dict().ok()) {
        if aa.len() > limits.max_blocks as usize {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: limits.max_blocks,
            }));
        }
        for (index, (key, value)) in aa.iter().enumerate() {
            if index % 256 == 0 {
                ctx.check()?;
            }
            if let Ok(action) = resolve(doc, value, depth)
                .and_then(|v| v.as_dict().map_err(|_| PdfError::Malformed))
            {
                let name = match key.as_slice() {
                    b"F" => "format",
                    b"K" => "keystroke",
                    b"V" => "validate",
                    b"C" => "calculate",
                    b"U" => "mouse_up",
                    _ => "other",
                };
                actions.insert(name.to_owned(), action);
            }
        }
    }
    let has_javascript = actions
        .values()
        .any(|d| get(doc, d, b"S", depth).and_then(|o| o.as_name().ok()) == Some(b"JavaScript"));
    Ok(ActionSummary {
        has_javascript,
        kinds: actions.into_keys().collect(),
    })
}
fn options(doc: &Document, object: Option<&Object>, depth: u16) -> Vec<ChoiceOption> {
    let Some(array) = object
        .and_then(|v| resolve(doc, v, depth).ok())
        .and_then(|v| v.as_array().ok())
    else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|item| {
            let item = resolve(doc, item, depth).ok()?;
            if let Object::Array(pair) = item {
                let export = decoded(doc, pair.first()?, depth)?;
                let display = decoded(doc, pair.get(1)?, depth)?;
                Some(ChoiceOption { export, display })
            } else {
                let value = decoded(doc, item, depth)?;
                Some(ChoiceOption {
                    export: value.clone(),
                    display: value,
                })
            }
        })
        .collect()
}

struct Walker<'a> {
    doc: &'a Document,
    limits: &'a Limits,
    ctx: &'a ToolCtx,
    pages: HashMap<ObjectId, u32>,
    widgets: HashMap<ObjectId, u32>,
    widget_order: HashMap<ObjectId, u32>,
    selected: HashSet<u32>,
    include_values: bool,
    visited: HashSet<ObjectId>,
    reachable_widgets: HashSet<ObjectId>,
    count: u32,
    listing: FormsListing,
}
impl Walker<'_> {
    fn check(&mut self, depth: usize) -> Result<(), PdfError> {
        self.ctx.check()?;
        if depth > MAX_FIELD_DEPTH || depth > usize::from(self.limits.max_nesting_depth) {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth.min(MAX_FIELD_DEPTH as u16),
            }));
        }
        self.count = self.count.saturating_add(1);
        if self.count > self.limits.max_blocks {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: self.limits.max_blocks,
            }));
        }
        if self.count as usize > MAX_FIELDS {
            self.listing.truncated = true;
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: MAX_FIELDS as u32,
            }));
        }
        Ok(())
    }
    fn widget(&self, object: &Object) -> Option<WidgetInfo> {
        let id = object.as_reference().ok();
        let field = resolve(self.doc, object, self.limits.max_nesting_depth)
            .ok()?
            .as_dict()
            .ok()?;
        let page = id
            .and_then(|id| self.widgets.get(&id).copied())
            .or_else(|| {
                field
                    .get(b"P")
                    .ok()
                    .and_then(|p| p.as_reference().ok())
                    .and_then(|id| self.pages.get(&id).copied())
            })?;
        let on_state = get(self.doc, field, b"AP", self.limits.max_nesting_depth)
            .and_then(|v| v.as_dict().ok())
            .and_then(|ap| get(self.doc, ap, b"N", self.limits.max_nesting_depth))
            .and_then(|v| v.as_dict().ok())
            .and_then(|n| {
                n.iter()
                    .find(|(key, _)| key.as_slice() != b"Off")
                    .and_then(|(key, _)| String::from_utf8(key.clone()).ok())
            });
        Some(WidgetInfo {
            page,
            rect: rect(self.doc, field, self.limits.max_nesting_depth)?,
            on_state,
        })
    }
    fn visit(
        &mut self,
        object: &Object,
        parent: &str,
        inherited: &Dictionary,
        depth: usize,
    ) -> Result<(), PdfError> {
        self.check(depth)?;
        if let Ok(id) = object.as_reference() {
            if !self.visited.insert(id) {
                self.listing
                    .warnings
                    .push("field_cycle: repeated field reference skipped".into());
                return Ok(());
            }
        }
        let field = match resolve(self.doc, object, self.limits.max_nesting_depth)
            .and_then(|v| v.as_dict().map_err(|_| PdfError::Malformed))
        {
            Ok(d) => d.clone(),
            Err(_) => {
                self.listing
                    .warnings
                    .push("invalid_field: malformed field skipped".into());
                return Ok(());
            }
        };
        for key in [b"T".as_slice(), b"TU", b"DA"] {
            if field.get(key).ok().and_then(|v| resolve(self.doc, v, self.limits.max_nesting_depth).ok())
                .is_some_and(|v| matches!(v, Object::String(bytes, _) | Object::Name(bytes) if bytes.len() > MAX_FILL_VALUE_BYTES)) {
                return Err(PdfError::InvalidParams("form field string exceeds the size limit".into()));
            }
        }
        let partial = field
            .get(b"T")
            .ok()
            .and_then(|v| decoded(self.doc, v, self.limits.max_nesting_depth))
            .unwrap_or_default();
        let name = if parent.is_empty() {
            partial
        } else if partial.is_empty() {
            parent.to_owned()
        } else {
            format!("{parent}.{partial}")
        };
        if name.len() > 65_536 {
            return Err(PdfError::Malformed);
        }
        let mut attrs = inherited.clone();
        for key in [
            b"FT".as_slice(),
            b"Ff",
            b"V",
            b"DV",
            b"Opt",
            b"MaxLen",
            b"DA",
            b"Q",
        ] {
            if let Ok(value) = field.get(key) {
                attrs.set(key, value.clone());
            }
        }
        let kid_array = field
            .get(b"Kids")
            .ok()
            .and_then(|v| resolve(self.doc, v, self.limits.max_nesting_depth).ok())
            .and_then(|v| v.as_array().ok());
        if kid_array.is_some_and(|kids| {
            kids.len() > MAX_FIELDS || kids.len() > self.limits.max_blocks as usize
        }) {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: self.limits.max_blocks.min(MAX_FIELDS as u32),
            }));
        }
        let kids = kid_array.cloned().unwrap_or_default();
        let mut widget_kids = Vec::new();
        let mut field_kids = Vec::new();
        for (index, kid) in kids.iter().enumerate() {
            if index % 256 == 0 {
                self.ctx.check()?;
            }
            let child = resolve(self.doc, kid, self.limits.max_nesting_depth)
                .ok()
                .and_then(|v| v.as_dict().ok());
            if child.is_some_and(|d| {
                !d.has(b"T")
                    && get(self.doc, d, b"Subtype", self.limits.max_nesting_depth)
                        .and_then(|v| v.as_name().ok())
                        == Some(b"Widget")
            }) {
                widget_kids.push(kid.clone());
            } else {
                field_kids.push(kid.clone());
            }
        }
        let subtype = get(self.doc, &field, b"Subtype", self.limits.max_nesting_depth)
            .and_then(|v| v.as_name().ok());
        if !name.is_empty()
            && (kids.is_empty() || !widget_kids.is_empty() || subtype == Some(b"Widget"))
        {
            let mut widgets = Vec::new();
            if subtype == Some(b"Widget") {
                if let Ok(id) = object.as_reference() {
                    self.reachable_widgets.insert(id);
                }
                if let Some(widget) = self.widget(object) {
                    widgets.push(widget);
                }
            }
            for kid in &widget_kids {
                self.check(depth + 1)?;
                if let Ok(id) = kid.as_reference() {
                    self.reachable_widgets.insert(id);
                }
                if let Some(widget) = self.widget(kid) {
                    widgets.push(widget);
                }
            }
            if widgets.iter().any(|w| self.selected.contains(&w.page)) {
                let raw_flags = get(self.doc, &attrs, b"Ff", self.limits.max_nesting_depth)
                    .and_then(|v| v.as_i64().ok())
                    .unwrap_or(0);
                let ft = get(self.doc, &attrs, b"FT", self.limits.max_nesting_depth)
                    .and_then(|v| v.as_name().ok());
                let kind = field_type(ft, raw_flags);
                // Bound arrays and strings before decoding potentially hostile field values.
                for key in [b"V".as_slice(), b"DV".as_slice(), b"Opt".as_slice()] {
                    let Some(root) = attrs.get(key).ok() else {
                        continue;
                    };
                    let mut pending = vec![(root, 0usize)];
                    let mut visited = 0usize;
                    while let Some((value, level)) = pending.pop() {
                        visited += 1;
                        if visited % 256 == 0 {
                            self.ctx.check()?;
                        }
                        if visited > MAX_FIELDS || visited > self.limits.max_blocks as usize {
                            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                                limit: self.limits.max_blocks.min(MAX_FIELDS as u32),
                            }));
                        }
                        if level > usize::from(self.limits.max_nesting_depth) {
                            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                                limit: self.limits.max_nesting_depth,
                            }));
                        }
                        match resolve(self.doc, value, self.limits.max_nesting_depth)? {
                            Object::Array(items) => {
                                if items.len() > MAX_FIELDS
                                    || items.len() > self.limits.max_blocks as usize
                                {
                                    return Err(PdfError::LimitExceeded(
                                        ReadError::TooManyBlocks {
                                            limit: self.limits.max_blocks.min(MAX_FIELDS as u32),
                                        },
                                    ));
                                }
                                pending.extend(items.iter().map(|item| (item, level + 1)));
                            }
                            Object::String(bytes, _) | Object::Name(bytes)
                                if bytes.len() > MAX_FILL_VALUE_BYTES =>
                            {
                                return Err(PdfError::InvalidParams(
                                    "form field value exceeds the size limit".into(),
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                let raw_value = attrs.get(b"V").ok();
                let signed = kind == Some(FieldType::Signature)
                    && raw_value
                        .and_then(|v| resolve(self.doc, v, self.limits.max_nesting_depth).ok())
                        .is_some_and(|v| v.as_dict().is_ok());
                let mut value = if self.include_values {
                    form_value(self.doc, raw_value, self.limits.max_nesting_depth)
                } else {
                    FormValue::Null(())
                };
                if kind == Some(FieldType::Checkbox) {
                    if let FormValue::Text(s) = &value {
                        value = FormValue::Bool(s != "Off");
                    }
                }
                let first = widgets
                    .iter()
                    .find(|widget| self.selected.contains(&widget.page))
                    .ok_or(PdfError::Malformed)?;
                let tab_order = std::iter::once(object)
                    .chain(widget_kids.iter())
                    .filter_map(|value| value.as_reference().ok())
                    .filter(|id| self.widgets.get(id) == Some(&first.page))
                    .filter_map(|id| self.widget_order.get(&id).copied())
                    .min();
                let mut actions = action_summary(self.doc, &field, self.limits, self.ctx)?;
                for kid in &widget_kids {
                    if let Ok(widget) = resolve(self.doc, kid, self.limits.max_nesting_depth)
                        .and_then(|v| v.as_dict().map_err(|_| PdfError::Malformed))
                    {
                        let child_actions =
                            action_summary(self.doc, widget, self.limits, self.ctx)?;
                        actions.has_javascript |= child_actions.has_javascript;
                        for kind in child_actions.kinds {
                            if !actions.kinds.contains(&kind) {
                                actions.kinds.push(kind);
                            }
                        }
                    }
                }
                actions.kinds.sort();
                let mut choices = options(
                    self.doc,
                    attrs.get(b"Opt").ok(),
                    self.limits.max_nesting_depth,
                );
                if kind == Some(FieldType::Radio) && choices.is_empty() {
                    choices = widgets
                        .iter()
                        .filter_map(|w| w.on_state.as_ref())
                        .map(|value| ChoiceOption {
                            export: value.clone(),
                            display: value.clone(),
                        })
                        .collect();
                }
                self.listing.fields.push(FieldInfo {
                    name: name.clone(),
                    field_type: kind,
                    value,
                    default_value: if self.include_values {
                        form_value(
                            self.doc,
                            attrs.get(b"DV").ok(),
                            self.limits.max_nesting_depth,
                        )
                    } else {
                        FormValue::Null(())
                    },
                    options: choices,
                    flags: flags(raw_flags),
                    max_length: get(self.doc, &attrs, b"MaxLen", self.limits.max_nesting_depth)
                        .and_then(|v| v.as_i64().ok())
                        .and_then(|v| u32::try_from(v).ok()),
                    page: first.page,
                    rect: first.rect,
                    widgets: widgets.clone(),
                    tooltip: field
                        .get(b"TU")
                        .ok()
                        .and_then(|v| decoded(self.doc, v, self.limits.max_nesting_depth)),
                    tab_order,
                    signed,
                    actions,
                });
            }
        }
        for child in &field_kids {
            self.visit(child, &name, &attrs, depth + 1)?;
        }
        Ok(())
    }
}

/// List fields on selected 1-based pages under the caller's resource limits.
pub fn list(
    pdf: &PdfDoc,
    pages: &[u32],
    include_values: bool,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<FormsListing, PdfError> {
    ctx.check()?;
    let doc = pdf.inner();
    let page_ids = collect_page_ids(doc, ctx)?;
    let mut page_map = HashMap::new();
    let mut widget_map = HashMap::new();
    let mut widget_order = HashMap::new();
    for (index, id) in page_ids.iter().enumerate() {
        ctx.check()?;
        let page = (index + 1) as u32;
        page_map.insert(*id, page);
        let dictionary = doc.get_dictionary(*id).map_err(|_| PdfError::Malformed)?;
        if let Some(annots) = get(doc, dictionary, b"Annots", limits.max_nesting_depth)
            .and_then(|v| v.as_array().ok())
        {
            if annots.len() > limits.max_blocks as usize {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: limits.max_blocks,
                }));
            }
            for (offset, annot) in annots.iter().enumerate() {
                if offset % 256 == 0 {
                    ctx.check()?;
                }
                if let Ok(id) = annot.as_reference() {
                    let is_widget = resolve(doc, annot, limits.max_nesting_depth)
                        .ok()
                        .and_then(|v| v.as_dict().ok())
                        .and_then(|d| get(doc, d, b"Subtype", limits.max_nesting_depth))
                        .and_then(|v| v.as_name().ok())
                        == Some(b"Widget");
                    if is_widget {
                        widget_map.entry(id).or_insert(page);
                        widget_order.entry(id).or_insert(offset as u32);
                    }
                }
            }
        }
    }
    let root = catalog_object_id(doc)?;
    let catalog = doc.get_dictionary(root).map_err(|_| PdfError::Malformed)?;
    let Some(form) =
        get(doc, catalog, b"AcroForm", limits.max_nesting_depth).and_then(|v| v.as_dict().ok())
    else {
        let mut listing = FormsListing::default();
        if !widget_map.is_empty() {
            listing
                .warnings
                .push("orphan_widgets: page widgets are not reachable from an AcroForm".into());
        }
        return Ok(listing);
    };
    let mut walker = Walker {
        doc,
        limits,
        ctx,
        pages: page_map,
        widgets: widget_map,
        widget_order,
        selected: pages.iter().copied().collect(),
        include_values,
        visited: HashSet::new(),
        reachable_widgets: HashSet::new(),
        count: 0,
        listing: FormsListing {
            xfa: form.has(b"XFA"),
            need_appearances: get(doc, form, b"NeedAppearances", limits.max_nesting_depth)
                .and_then(|v| v.as_bool().ok())
                .unwrap_or(false),
            ..FormsListing::default()
        },
    };
    if let Some(fields) =
        get(doc, form, b"Fields", limits.max_nesting_depth).and_then(|v| v.as_array().ok())
    {
        for field in fields {
            walker.visit(field, "", &Dictionary::new(), 0)?;
        }
    }
    if walker.listing.xfa {
        walker
            .listing
            .warnings
            .push("xfa_ignored: dynamic XFA content is not processed".into());
    }
    if walker
        .widgets
        .keys()
        .any(|id| !walker.reachable_widgets.contains(id))
    {
        walker
            .listing
            .warnings
            .push("orphan_widgets: page widgets are not reachable from the field tree".into());
    }
    Ok(walker.listing)
}
