//! Bounded, network-free XFDF form-data handling.

use super::MAX_DATA_STRING_BYTES;

use std::collections::BTreeMap;

use ariad_core::tools::params::form_model::{FormValue, MAX_FIELDS};
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
};

use super::{FormRecord, MAX_DATA_BYTES, MAX_DATA_DEPTH, check_data_size};
use crate::{PdfError, ToolCtx};

const MAX_EVENTS: usize = 1_000_000;
const XFDF_NS: &str = "http://ns.adobe.com/xfdf/";
fn invalid() -> PdfError {
    PdfError::InvalidParams("invalid XFDF input".into())
}

fn field_name(element: &BytesStart<'_>) -> Result<String, PdfError> {
    for attribute in element.attributes() {
        let attr = attribute.map_err(|_| invalid())?;
        if attr.key.as_ref() == "name" {
            let value = attr
                .normalized_value_with(
                    quick_xml::XmlVersion::Implicit1_0,
                    1,
                    quick_xml::escape::resolve_xml_entity,
                )
                .map_err(|_| invalid())?
                .into_owned();
            if value.is_empty() || value.len() > MAX_DATA_STRING_BYTES {
                return Err(invalid());
            }
            return Ok(value);
        }
    }
    Err(invalid())
}
fn append_value(target: &mut String, text: &str) -> Result<(), PdfError> {
    if target.len().saturating_add(text.len()) > MAX_DATA_STRING_BYTES {
        return Err(invalid());
    }
    target.push_str(text);
    Ok(())
}

/// Read form fields from XFDF. DTDs and non-standard entity references are rejected.
pub fn read_forms(bytes: &[u8], ctx: &ToolCtx) -> Result<(FormRecord, Vec<String>), PdfError> {
    check_data_size(bytes)?;
    ctx.check()?;
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let xml = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    if xml.contains('\0') {
        return Err(invalid());
    }
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<String> = Vec::new();
    let mut field_path: Vec<String> = Vec::new();
    let mut field_depths: Vec<usize> = Vec::new();
    let mut values: Vec<Vec<String>> = Vec::new();
    let mut current: Option<String> = None;
    let mut value_depth = 0usize;
    let mut rich = false;
    let mut warnings = Vec::new();
    let mut result = FormRecord::new();
    let mut root_seen = false;
    let mut events = 0usize;
    loop {
        events += 1;
        if events > MAX_EVENTS {
            return Err(invalid());
        }
        if events.is_multiple_of(256) {
            ctx.check()?;
        }
        let (namespace, event) = reader.read_resolved_event().map_err(|_| invalid())?;
        match event {
            Event::Start(e) if !root_seen => {
                let name = e.local_name();
                if name.as_ref() != "xfdf"
                    || !matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == XFDF_NS)
                {
                    return Err(invalid());
                }
                root_seen = true;
                stack.push("xfdf".to_owned());
            }
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_owned();
                if stack.is_empty() || stack.len() >= MAX_DATA_DEPTH {
                    return Err(invalid());
                }
                let same_ns =
                    matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == XFDF_NS);
                if same_ns
                    && name == "field"
                    && stack
                        .last()
                        .is_some_and(|tag| tag == "fields" || tag == "field")
                {
                    field_path.push(field_name(&e)?);
                    field_depths.push(stack.len());
                    values.push(Vec::new());
                    if field_path.len() > MAX_DATA_DEPTH
                        || field_path.join(".").len() > MAX_DATA_STRING_BYTES
                    {
                        return Err(invalid());
                    }
                } else if same_ns
                    && (name == "value" || name == "value-richtext")
                    && !field_path.is_empty()
                    && stack.last().is_some_and(|tag| tag == "field")
                {
                    if current.is_some() {
                        return Err(invalid());
                    }
                    current = Some(String::new());
                    value_depth = stack.len();
                    rich = name == "value-richtext";
                    if rich
                        && !warnings
                            .iter()
                            .any(|w: &String| w.starts_with("rich_text_flattened:"))
                    {
                        warnings
                            .push("rich_text_flattened: rich text converted to plain text".into());
                    }
                }
                stack.push(name);
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_owned();
                if stack.pop().as_ref() != Some(&name) {
                    return Err(invalid());
                }
                if current.is_some()
                    && stack.len() == value_depth
                    && (name == "value" || name == "value-richtext")
                {
                    values
                        .last_mut()
                        .ok_or_else(invalid)?
                        .push(current.take().unwrap_or_default());
                    rich = false;
                }
                if name == "field" && field_depths.last() == Some(&stack.len()) {
                    field_depths.pop();
                    let name = field_path.join(".");
                    let found = values.pop().ok_or_else(invalid)?;
                    if !found.is_empty() {
                        if result.len() >= MAX_FIELDS && !result.contains_key(&name) {
                            return Err(invalid());
                        }
                        let value = if found.len() == 1 {
                            FormValue::Text(found.into_iter().next().unwrap_or_default())
                        } else {
                            FormValue::Multi(found)
                        };
                        result.insert(name, value);
                    }
                    field_path.pop();
                }
            }
            Event::Text(e) => {
                let text = e.xml10_content();
                if let Some(value) = current.as_mut() {
                    append_value(value, &text)?;
                } else if stack.is_empty() && !text.trim().is_empty() {
                    return Err(invalid());
                }
            }
            Event::CData(e) => {
                if stack.is_empty() {
                    return Err(invalid());
                }
                if let Some(value) = current.as_mut() {
                    append_value(value, &e.xml10_content())?;
                }
            }
            Event::GeneralRef(e) => {
                let resolved = if let Some(c) = e.resolve_char_ref().map_err(|_| invalid())? {
                    c.to_string()
                } else {
                    quick_xml::escape::resolve_xml_entity(e.as_ref())
                        .ok_or_else(invalid)?
                        .to_owned()
                };
                if let Some(value) = current.as_mut() {
                    append_value(value, &resolved)?;
                }
            }
            Event::DocType(_) | Event::Empty(_) => return Err(invalid()),
            Event::Eof => {
                if !root_seen || !stack.is_empty() || current.is_some() || rich {
                    return Err(invalid());
                }
                break;
            }
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
        }
    }
    Ok((result, warnings))
}

#[derive(Default)]
struct Tree {
    value: Option<FormValue>,
    children: BTreeMap<String, Tree>,
}
fn valid_xml(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{a}' | '\u{d}')
        || ('\u{20}'..='\u{d7ff}').contains(&c)
        || ('\u{e000}'..='\u{fffd}').contains(&c)
        || ('\u{10000}'..='\u{10ffff}').contains(&c)
}
fn escaped(text: &str, output: &mut String, dropped: &mut bool) -> Result<(), PdfError> {
    if text.len() > MAX_DATA_STRING_BYTES {
        return Err(invalid());
    }
    let sanitized: String = text
        .chars()
        .filter(|&c| {
            if valid_xml(c) {
                true
            } else {
                *dropped = true;
                false
            }
        })
        .collect();
    output.push_str(&quick_xml::escape::escape(&sanitized));
    Ok(())
}
fn emit(
    tree: &BTreeMap<String, Tree>,
    output: &mut String,
    depth: usize,
    dropped: &mut bool,
) -> Result<(), PdfError> {
    if depth > MAX_DATA_DEPTH {
        return Err(invalid());
    }
    for (name, node) in tree {
        output.push_str("<field name=\"");
        escaped(name, output, dropped)?;
        output.push_str("\">");
        if let Some(value) = &node.value {
            let parts: Vec<String> = match value {
                FormValue::Text(s) => vec![s.clone()],
                FormValue::Bool(b) => vec![b.to_string()],
                FormValue::Multi(v) => v.clone(),
                FormValue::Null(()) => vec![String::new()],
            };
            for part in parts {
                output.push_str("<value>");
                escaped(&part, output, dropped)?;
                output.push_str("</value>");
            }
        }
        emit(&node.children, output, depth + 1, dropped)?;
        output.push_str("</field>");
        if output.len() > MAX_DATA_BYTES {
            return Err(invalid());
        }
    }
    Ok(())
}
/// Write a deterministic XFDF record with a filename-only source reference.
pub fn write_forms(record: &FormRecord, filename: &str) -> Result<Vec<u8>, PdfError> {
    Ok(write_forms_with_warnings(record, filename)?.0)
}

/// Write XFDF and report whether XML-invalid characters were dropped.
pub fn write_forms_with_warnings(
    record: &FormRecord,
    filename: &str,
) -> Result<(Vec<u8>, bool), PdfError> {
    if record.len() > MAX_FIELDS {
        return Err(invalid());
    }
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or_default();
    if basename.len() > MAX_DATA_STRING_BYTES {
        return Err(invalid());
    }
    let mut root = Tree::default();
    for (name, value) in record {
        if name.len() > MAX_DATA_STRING_BYTES {
            return Err(invalid());
        }
        let mut current = &mut root;
        for (depth, part) in name.split('.').enumerate() {
            if part.is_empty() || depth >= MAX_DATA_DEPTH {
                return Err(invalid());
            }
            current = current.children.entry(part.into()).or_default();
        }
        current.value = Some(value.clone());
    }
    let mut output = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\"><f href=\"",
    );
    let mut dropped = false;
    escaped(basename, &mut output, &mut dropped)?;
    output.push_str("\"/><fields>");
    emit(&root.children, &mut output, 0, &mut dropped)?;
    output.push_str("</fields></xfdf>");
    check_data_size(output.as_bytes())?;
    Ok((output.into_bytes(), dropped))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_reject_entities() {
        let input = FormRecord::from([("person.name".into(), FormValue::Text("Hà & An".into()))]);
        let bytes = write_forms(&input, "/tmp/form.pdf").unwrap();
        assert_eq!(read_forms(&bytes, &ToolCtx::new()).unwrap().0, input);
        assert!(read_forms(b"<!DOCTYPE xfdf [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><xfdf xmlns='http://ns.adobe.com/xfdf/'>&x;</xfdf>", &ToolCtx::new()).is_err());
    }
}
