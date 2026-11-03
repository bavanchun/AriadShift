//! Bounded XFDF annotation parsing and serialization.

use super::{MAX_DATA_DEPTH, check_data_size};
use crate::{PdfError, ToolCtx};
use ariad_core::tools::params::annotations::{
    AnnotationInfo, AnnotationItem, AnnotationType, MAX_ANNOTATION_ITEMS,
};
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
};
use std::collections::{BTreeMap, HashSet};

const NS: &str = "http://ns.adobe.com/xfdf/";
const MAX_EVENTS: usize = 1_000_000;
fn invalid() -> PdfError {
    PdfError::InvalidParams("invalid XFDF annotation data".into())
}
fn kind(name: &str) -> Option<&'static str> {
    Some(match name {
        "highlight" => "highlight",
        "underline" => "underline",
        "strikeout" => "strikeout",
        "squiggly" => "squiggly",
        "text" => "note",
        "freetext" => "free_text",
        "ink" => "ink",
        "line" => "line",
        "square" => "rectangle",
        "circle" => "ellipse",
        "polygon" => "polygon",
        "polyline" => "polyline",
        "stamp" => "stamp",
        "link" => "link",
        _ => return None,
    })
}
fn tag(kind: AnnotationType) -> &'static str {
    match kind {
        AnnotationType::Highlight => "highlight",
        AnnotationType::Underline => "underline",
        AnnotationType::Strikeout => "strikeout",
        AnnotationType::Squiggly => "squiggly",
        AnnotationType::Note => "text",
        AnnotationType::FreeText => "freetext",
        AnnotationType::Ink => "ink",
        AnnotationType::Line | AnnotationType::Arrow => "line",
        AnnotationType::Rectangle => "square",
        AnnotationType::Ellipse => "circle",
        AnnotationType::Polygon => "polygon",
        AnnotationType::Polyline => "polyline",
        AnnotationType::Stamp => "stamp",
        AnnotationType::Link => "link",
    }
}
fn attrs(start: &BytesStart<'_>) -> Result<BTreeMap<String, String>, PdfError> {
    let mut result = BTreeMap::new();
    for a in start.attributes() {
        let a = a.map_err(|_| invalid())?;
        let key = a.key.as_ref().to_owned();
        let val = a
            .normalized_value_with(
                quick_xml::XmlVersion::Implicit1_0,
                1,
                quick_xml::escape::resolve_xml_entity,
            )
            .map_err(|_| invalid())?
            .into_owned();
        if val.len() > 1_048_576 || result.insert(key, val).is_some() {
            return Err(invalid());
        }
    }
    Ok(result)
}
fn numbers(s: &str) -> Result<Vec<f64>, PdfError> {
    let values = s
        .split([',', ';', ' '])
        .filter(|s| !s.is_empty())
        .map(|v| v.parse::<f64>().map_err(|_| invalid()))
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() > 80_000 || values.iter().any(|v| !v.is_finite()) {
        return Err(invalid());
    }
    Ok(values)
}
fn points(s: &str) -> Result<Vec<[f64; 2]>, PdfError> {
    let nums = numbers(s)?;
    if nums.len() % 2 != 0 {
        return Err(invalid());
    }
    Ok(nums
        .as_chunks::<2>()
        .0
        .iter()
        .map(|x| [x[0], x[1]])
        .collect())
}
fn from_attrs(name: &str, a: &BTreeMap<String, String>) -> Result<AnnotationItem, PdfError> {
    let mut value = serde_json::json!({"type":kind(name).ok_or_else(invalid)?,"page":a.get("page").ok_or_else(invalid)?.parse::<u32>().map_err(|_| invalid())?.checked_add(1).ok_or_else(invalid)?});
    let obj = value.as_object_mut().ok_or_else(invalid)?;
    for (source, target) in [
        ("name", "id"),
        ("color", "color"),
        ("interior-color", "fill"),
        ("title", "author"),
        ("subject", "subject"),
        ("uri", "uri"),
    ] {
        if let Some(v) = a.get(source) {
            obj.insert(target.into(), v.clone().into());
        }
    }
    for (source, target) in [
        ("opacity", "opacity"),
        ("width", "line_width"),
        ("font-size", "font_size"),
    ] {
        if let Some(v) = a.get(source) {
            obj.insert(
                target.into(),
                serde_json::json!(v.parse::<f64>().map_err(|_| invalid())?),
            );
        }
    }
    if let Some(v) = a.get("rect") {
        let n = numbers(v)?;
        if n.len() != 4 {
            return Err(invalid());
        }
        obj.insert("rect".into(), serde_json::json!(n));
    }
    if let Some(v) = a.get("coords") {
        let n = numbers(v)?;
        if n.len() % 8 != 0 {
            return Err(invalid());
        }
        obj.insert(
            "quads".into(),
            serde_json::json!(n.as_chunks::<8>().0.iter().collect::<Vec<_>>()),
        );
    }
    if let (Some(a), Some(b)) = (a.get("start"), a.get("end")) {
        let x = points(a)?;
        let y = points(b)?;
        if x.len() != 1 || y.len() != 1 {
            return Err(invalid());
        }
        obj.insert("points".into(), serde_json::json!([x[0], y[0]]));
    }
    if let Some(v) = a.get("vertices") {
        obj.insert("points".into(), serde_json::json!(points(v)?));
    }
    if name == "line"
        && a.get("tail").is_some_and(|s| {
            s.eq_ignore_ascii_case("openarrow") || s.eq_ignore_ascii_case("closedarrow")
        })
    {
        obj.insert("type".into(), "arrow".into());
    }
    if let Some(v) = a.get("flags") {
        let set: HashSet<_> = v.split(',').map(str::trim).collect();
        obj.insert("flags".into(), serde_json::json!({"hidden":set.contains("hidden"),"print":set.contains("print"),"no_view":set.contains("noview"),"no_rotate":set.contains("norotate"),"locked":set.contains("locked"),"read_only":set.contains("readonly")}));
    }
    if name == "stamp"
        && let Some(v) = a.get("icon")
    {
        let stamp = match v.as_str() {
            "Approved" => Some("approved"),
            "Experimental" => Some("experimental"),
            "NotApproved" => Some("not_approved"),
            "AsIs" => Some("as_is"),
            "Expired" => Some("expired"),
            "NotForPublicRelease" => Some("not_for_public_release"),
            "Confidential" => Some("confidential"),
            "Final" => Some("final"),
            "Sold" => Some("sold"),
            "Departmental" => Some("departmental"),
            "ForComment" => Some("for_comment"),
            "TopSecret" => Some("top_secret"),
            "Draft" => Some("draft"),
            "ForPublicRelease" => Some("for_public_release"),
            _ => None,
        };
        if let Some(stamp) = stamp {
            obj.insert("stamp".into(), stamp.into());
        } else {
            obj.insert("text".into(), v.clone().into());
        }
    }
    if let Some(v) = a.get("dest-page") {
        obj.insert(
            "dest_page".into(),
            serde_json::json!(
                v.parse::<u32>()
                    .map_err(|_| invalid())?
                    .checked_add(1)
                    .ok_or_else(invalid)?
            ),
        );
    }
    serde_json::from_value(value).map_err(|_| invalid())
}

/// Read supported XFDF annotations; external file references are ignored.
pub fn read(bytes: &[u8], ctx: &ToolCtx) -> Result<(Vec<AnnotationItem>, Vec<String>), PdfError> {
    check_data_size(bytes)?;
    ctx.check()?;
    let xml = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes))
        .map_err(|_| invalid())?;
    if xml.contains('\0') {
        return Err(invalid());
    }
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<String> = Vec::new();
    let mut root = false;
    let mut events = 0usize;
    let mut current: Option<AnnotationItem> = None;
    let mut result = Vec::new();
    let mut warnings = Vec::new();
    let mut unknown = HashSet::new();
    let mut text = String::new();
    let mut strokes = Vec::new();
    loop {
        events += 1;
        if events > MAX_EVENTS {
            return Err(invalid());
        }
        if events.is_multiple_of(256) {
            ctx.check()?;
        }
        let (ns, event) = reader.read_resolved_event().map_err(|_| invalid())?;
        match event {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_owned();
                if stack.len() >= MAX_DATA_DEPTH {
                    return Err(invalid());
                }
                let same = matches!(ns,ResolveResult::Bound(n) if n.as_ref()==NS);
                if !root {
                    if name != "xfdf" || !same {
                        return Err(invalid());
                    }
                    root = true;
                } else if stack.is_empty() {
                    return Err(invalid());
                } else if same && stack.last().is_some_and(|s| s == "annots") {
                    if kind(&name).is_some() {
                        if result.len() >= MAX_ANNOTATION_ITEMS {
                            return Err(invalid());
                        }
                        current = Some(from_attrs(&name, &attrs(&e)?)?);
                        strokes.clear();
                    } else if unknown.insert(name.clone()) {
                        warnings.push(format!(
                            "annotation_type_skipped: unsupported XFDF element {name}"
                        ));
                    }
                }
                if current.is_some()
                    && matches!(
                        name.as_str(),
                        "contents" | "contents-richtext" | "gesture" | "vertices"
                    )
                {
                    text.clear();
                }
                stack.push(name);
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_owned();
                if stack.pop().as_ref() != Some(&name) {
                    return Err(invalid());
                }
                if let Some(item) = current.as_mut() {
                    match name.as_str() {
                        "contents" | "contents-richtext" => {
                            item.contents = Some(text.clone());
                            if name == "contents-richtext" {
                                warnings.push(
                                    "rich_text_flattened: rich text converted to plain text".into(),
                                );
                            }
                        }
                        "gesture" => strokes.push(points(&text)?),
                        "vertices" => item.points = Some(points(&text)?),
                        _ => {}
                    }
                    if stack.last().is_some_and(|s| s == "annots") {
                        if !strokes.is_empty() {
                            item.strokes = Some(std::mem::take(&mut strokes));
                        }
                        if item.annotation_type == AnnotationType::FreeText {
                            item.text = item.contents.clone();
                        }
                        item.validate().map_err(PdfError::InvalidParams)?;
                        result.push(current.take().ok_or_else(invalid)?);
                    }
                }
            }
            Event::Text(e) => {
                if current.is_some()
                    && stack.iter().any(|s| {
                        matches!(
                            s.as_str(),
                            "contents" | "contents-richtext" | "gesture" | "vertices"
                        )
                    })
                {
                    let s = e.xml10_content();
                    if text.len().saturating_add(s.len()) > 1_048_576 {
                        return Err(invalid());
                    }
                    text.push_str(&s);
                } else if stack.is_empty() && !e.xml10_content().trim().is_empty() {
                    return Err(invalid());
                }
            }
            Event::CData(e) => {
                if current.is_some() {
                    let s = e.xml10_content();
                    if text.len().saturating_add(s.len()) > 1_048_576 {
                        return Err(invalid());
                    }
                    text.push_str(&s);
                }
            }
            Event::GeneralRef(e) => {
                let value = if let Some(c) = e.resolve_char_ref().map_err(|_| invalid())? {
                    c.to_string()
                } else {
                    quick_xml::escape::resolve_xml_entity(e.as_ref())
                        .ok_or_else(invalid)?
                        .to_owned()
                };
                if current.is_some() {
                    if text.len().saturating_add(value.len()) > 1_048_576 {
                        return Err(invalid());
                    }
                    text.push_str(&value);
                }
            }
            Event::DocType(_) | Event::Empty(_) => return Err(invalid()),
            Event::Eof => {
                if !root || !stack.is_empty() || current.is_some() {
                    return Err(invalid());
                }
                break;
            }
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
        }
    }
    Ok((result, warnings))
}
fn valid_xml(value: &str) -> bool {
    value.chars().all(|c| {
        matches!(c, '\t' | '\n' | '\r')
            || ('\u{20}'..='\u{d7ff}').contains(&c)
            || ('\u{e000}'..='\u{fffd}').contains(&c)
            || ('\u{10000}'..='\u{10ffff}').contains(&c)
    })
}
fn attr(out: &mut String, key: &str, value: &str) -> Result<(), PdfError> {
    if !valid_xml(value) {
        return Err(invalid());
    }
    out.push(' ');
    out.push_str(key);
    out.push_str("=\"");
    out.push_str(&quick_xml::escape::escape(value));
    out.push('"');
    Ok(())
}
fn coords<'a>(values: impl IntoIterator<Item = &'a f64>) -> String {
    values
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
/// Write annotation metadata to XFDF without resolving any external resources.
pub fn write(items: &[AnnotationInfo], filename: &str, ctx: &ToolCtx) -> Result<Vec<u8>, PdfError> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\"><f",
    );
    attr(
        &mut out,
        "href",
        filename.rsplit(['/', '\\']).next().unwrap_or_default(),
    )?;
    out.push_str("/><annots>");
    for info in items {
        ctx.check()?;
        let Some(kind) = info.annotation_type else {
            continue;
        };
        let name = tag(kind);
        out.push('<');
        out.push_str(name);
        attr(
            &mut out,
            "page",
            &info.page.checked_sub(1).ok_or_else(invalid)?.to_string(),
        )?;
        attr(&mut out, "name", &info.id)?;
        if let Some(v) = info.rect {
            attr(&mut out, "rect", &coords(&v))?;
        }
        if let Some(v) = &info.quads {
            attr(&mut out, "coords", &coords(v.iter().flatten()))?;
        }
        if let Some(v) = &info.points {
            if matches!(kind, AnnotationType::Line | AnnotationType::Arrow) && v.len() == 2 {
                attr(&mut out, "start", &coords(&v[0]))?;
                attr(&mut out, "end", &coords(&v[1]))?;
            } else {
                attr(&mut out, "vertices", &coords(v.iter().flatten()))?;
            }
        }
        if kind == AnnotationType::Arrow {
            attr(&mut out, "tail", "OpenArrow")?;
        }
        for (key, value) in [
            ("color", &info.color),
            ("interior-color", &info.fill),
            ("title", &info.author),
            ("subject", &info.subject),
            ("uri", &info.uri),
        ] {
            if let Some(value) = value {
                attr(&mut out, key, value)?;
            }
        }
        let mut flags = Vec::new();
        if info.flags.hidden {
            flags.push("hidden");
        }
        if info.flags.print {
            flags.push("print");
        }
        if info.flags.no_view {
            flags.push("noview");
        }
        if info.flags.no_rotate {
            flags.push("norotate");
        }
        if info.flags.locked {
            flags.push("locked");
        }
        if info.flags.read_only {
            flags.push("readonly");
        }
        attr(&mut out, "flags", &flags.join(","))?;
        if kind == AnnotationType::Stamp {
            use ariad_core::tools::params::annotations::StampName;
            let icon = info.stamp.map(|s| match s {
                StampName::Approved => "Approved",
                StampName::Experimental => "Experimental",
                StampName::NotApproved => "NotApproved",
                StampName::AsIs => "AsIs",
                StampName::Expired => "Expired",
                StampName::NotForPublicRelease => "NotForPublicRelease",
                StampName::Confidential => "Confidential",
                StampName::Final => "Final",
                StampName::Sold => "Sold",
                StampName::Departmental => "Departmental",
                StampName::ForComment => "ForComment",
                StampName::TopSecret => "TopSecret",
                StampName::Draft => "Draft",
                StampName::ForPublicRelease => "ForPublicRelease",
            });
            if let Some(icon) = icon.or(info.text.as_deref()) {
                attr(&mut out, "icon", icon)?;
            }
        }
        attr(&mut out, "opacity", &info.opacity.to_string())?;
        attr(&mut out, "width", &info.line_width.to_string())?;
        attr(&mut out, "font-size", &info.font_size.to_string())?;
        if let Some(v) = info.dest_page {
            attr(
                &mut out,
                "dest-page",
                &v.checked_sub(1).ok_or_else(invalid)?.to_string(),
            )?;
        }
        out.push('>');
        if let Some(contents) = info.contents.as_ref().or(info.text.as_ref()) {
            if !valid_xml(contents) {
                return Err(invalid());
            }
            out.push_str("<contents>");
            out.push_str(&quick_xml::escape::escape(contents));
            out.push_str("</contents>");
        }
        if let Some(strokes) = &info.strokes {
            out.push_str("<inklist>");
            for stroke in strokes {
                out.push_str("<gesture>");
                out.push_str(&coords(stroke.iter().flatten()));
                out.push_str("</gesture>");
            }
            out.push_str("</inklist>");
        }
        out.push_str("</");
        out.push_str(name);
        out.push('>');
        check_data_size(out.as_bytes())?;
    }
    out.push_str("</annots></xfdf>");
    check_data_size(out.as_bytes())?;
    Ok(out.into_bytes())
}
