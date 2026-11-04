//! Bounded removal of matching PDF object strings.

use std::collections::{HashSet, VecDeque};

use lopdf::{Object, ObjectId, StringFormat};

use super::matching::{MatchBudget, Matchers};
use crate::{PdfDoc, PdfError, ToolCtx};

const MAX_DEPTH: usize = 128;
const MAX_OBJECTS: usize = 1_000_000;

/// Remove references to opaque embedded-file and XFA payloads before graph inspection.
/// The caller must still inspect all remaining streams and verify the final rewrite.
pub(super) fn remove_opaque_carriers(pdf: &mut PdfDoc, ctx: &ToolCtx) -> Result<usize, PdfError> {
    ctx.check()?;
    let mut candidate = pdf.clone();
    let doc = candidate.inner_mut()?;
    let mut visited = 0usize;
    let mut removed = 0usize;
    for object in doc.objects.values_mut() {
        remove_opaque_keys(object, 0, &mut visited, &mut removed, ctx)?;
    }
    for (_, object) in doc.trailer.iter_mut() {
        remove_opaque_keys(object, 0, &mut visited, &mut removed, ctx)?;
    }
    ctx.check()?;
    *pdf = candidate;
    Ok(removed)
}

fn remove_opaque_keys(
    object: &mut Object,
    depth: usize,
    visited: &mut usize,
    removed: &mut usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    *visited = visited.checked_add(1).ok_or(PdfError::Malformed)?;
    if *visited > MAX_OBJECTS || depth > MAX_DEPTH {
        return Err(PdfError::Malformed);
    }
    if visited.is_multiple_of(128) {
        ctx.check()?;
    }
    match object {
        Object::Array(items) => {
            for item in items {
                remove_opaque_keys(item, depth + 1, visited, removed, ctx)?;
            }
        }
        Object::Dictionary(dict) => remove_opaque_dict(dict, depth, visited, removed, ctx)?,
        Object::Stream(stream) => {
            remove_opaque_dict(&mut stream.dict, depth, visited, removed, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

fn remove_opaque_dict(
    dict: &mut lopdf::Dictionary,
    depth: usize,
    visited: &mut usize,
    removed: &mut usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    for key in [b"XFA".as_slice(), b"EmbeddedFiles", b"AF", b"EF", b"RF"] {
        if dict.remove(key).is_some() {
            *removed = removed.checked_add(1).ok_or(PdfError::Malformed)?;
        }
    }
    for (_, value) in dict.iter_mut() {
        remove_opaque_keys(value, depth + 1, visited, removed, ctx)?;
    }
    Ok(())
}

/// This stage does not inspect stream payloads and cannot establish graph verification.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StringScrubStats {
    pub scrubbed: usize,
    pub uninspected_streams: usize,
}

/// Clears whole matching strings in a candidate copy. Failures leave `pdf` unchanged.
pub fn scrub_object_strings(
    pdf: &mut PdfDoc,
    matchers: &Matchers,
    budget: &mut MatchBudget,
    ctx: &ToolCtx,
) -> Result<StringScrubStats, PdfError> {
    ctx.check()?;
    let mut candidate = pdf.clone();
    let doc = candidate.inner_mut()?;
    let mut pending = VecDeque::new();
    let mut state = Walker {
        matchers,
        budget,
        ctx,
        pending: &mut pending,
        stats: StringScrubStats::default(),
        visits: 0,
    };
    for (_, value) in doc.trailer.iter_mut() {
        state.walk(value, 0)?;
    }
    let mut visited = HashSet::new();
    while let Some((id, depth)) = state.pending.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        let value = doc.objects.get_mut(&id).ok_or(PdfError::Malformed)?;
        state.walk(value, depth)?;
    }
    let stats = state.stats;
    ctx.check()?;
    *pdf = candidate;
    Ok(stats)
}

struct Walker<'a> {
    matchers: &'a Matchers,
    budget: &'a mut MatchBudget,
    ctx: &'a ToolCtx,
    pending: &'a mut VecDeque<(ObjectId, usize)>,
    stats: StringScrubStats,
    visits: usize,
}

impl Walker<'_> {
    fn walk(&mut self, value: &mut Object, depth: usize) -> Result<(), PdfError> {
        self.visits += 1;
        if self.visits > MAX_OBJECTS || depth > MAX_DEPTH {
            return Err(PdfError::Malformed);
        }
        if self.visits.is_multiple_of(128) {
            self.ctx.check()?;
        }
        match value {
            Object::Reference(id) => self.pending.push_back((*id, depth + 1)),
            Object::Array(items) => {
                for item in items {
                    self.walk(item, depth + 1)?;
                }
            }
            Object::Dictionary(dict) => {
                let scrubbed_before = self.stats.scrubbed;
                for (_, item) in dict.iter_mut() {
                    self.walk(item, depth + 1)?;
                }
                if self.stats.scrubbed > scrubbed_before {
                    dict.remove(b"AP");
                }
            }
            Object::Stream(stream) => {
                self.stats.uninspected_streams += 1;
                for (_, item) in stream.dict.iter_mut() {
                    self.walk(item, depth + 1)?;
                }
                if stream
                    .dict
                    .get(b"Type")
                    .is_ok_and(|t| matches!(t.as_name(), Ok(b"Metadata")))
                {
                    let raw = stream
                        .decompressed_content()
                        .unwrap_or_else(|_| stream.content.clone());
                    let mut has_match = if let Ok(text) = std::str::from_utf8(&raw) {
                        self.matchers
                            .find(text, self.budget)
                            .is_ok_and(|m| !m.is_empty())
                    } else {
                        false
                    };
                    if !has_match && raw.len() >= 2 && raw.len() % 2 == 0 {
                        let u16s_be: Vec<u16> = raw
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|&[c0, c1]| u16::from_be_bytes([c0, c1]))
                            .collect();
                        if let Ok(text_be) = String::from_utf16(&u16s_be)
                            && self
                                .matchers
                                .find(&text_be, self.budget)
                                .is_ok_and(|m| !m.is_empty())
                        {
                            has_match = true;
                        }
                        if !has_match {
                            let u16s_le: Vec<u16> = raw
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|&[c0, c1]| u16::from_le_bytes([c0, c1]))
                                .collect();
                            if let Ok(text_le) = String::from_utf16(&u16s_le)
                                && self
                                    .matchers
                                    .find(&text_le, self.budget)
                                    .is_ok_and(|m| !m.is_empty())
                            {
                                has_match = true;
                            }
                        }
                    }
                    if has_match {
                        stream.content.clear();
                        stream.dict.remove(b"Filter");
                        self.stats.scrubbed += 1;
                    }
                }
                if stream
                    .dict
                    .get(b"Subtype")
                    .is_ok_and(|s| matches!(s.as_name(), Ok(b"Form")))
                {
                    let raw = stream
                        .decompressed_content()
                        .unwrap_or_else(|_| stream.content.clone());
                    let has_match = if let Ok(text) = std::str::from_utf8(&raw) {
                        self.matchers
                            .find(text, self.budget)
                            .is_ok_and(|m| !m.is_empty())
                    } else {
                        false
                    };
                    if has_match {
                        stream.content.clear();
                        stream.dict.remove(b"Filter");
                        self.stats.scrubbed += 1;
                    }
                }
            }
            Object::String(bytes, _) => {
                let text = decode_string(bytes)?;
                let found = self.matchers.find(&text, self.budget);
                self.ctx.check()?;
                if !found
                    .map_err(|error| PdfError::InvalidParams(error.into()))?
                    .is_empty()
                {
                    *value = Object::String(Vec::new(), StringFormat::Literal);
                    self.stats.scrubbed += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn decode_string(bytes: &[u8]) -> Result<String, PdfError> {
    if let Some((data, little)) = bytes
        .strip_prefix(b"\xfe\xff")
        .map(|v| (v, false))
        .or_else(|| bytes.strip_prefix(b"\xff\xfe").map(|v| (v, true)))
    {
        if data.len() % 2 != 0 {
            return Err(PdfError::Malformed);
        }
        let units = data.as_chunks::<2>().0.iter().map(|pair| {
            if little {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        });
        return char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(|_| PdfError::Malformed);
    }
    lopdf::decode_text_string(&Object::String(bytes.to_vec(), StringFormat::Literal))
        .map_err(|_| PdfError::Malformed)
}

/// Traverses all reachable objects from the trailer, including decoded streams,
/// and verifies that no requested target text appears anywhere in the graph.
pub fn verify_object_graph(
    bytes: &[u8],
    params: &ariad_core::tools::params::redact::RedactParams,
    limits: &ariad_core::limits::Limits,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    use ariad_core::tools::params::{
        redact::RedactionTarget,
        redact_internal::RedactionCheckStatus::{Fail, Pass, Unavailable},
    };
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    if !params.scope.is_complete() {
        return Ok(Unavailable);
    }
    let has_text_targets = params.targets.iter().any(|t| {
        matches!(
            t,
            RedactionTarget::Exact { .. }
                | RedactionTarget::Regex { .. }
                | RedactionTarget::Preset { .. }
        )
    });
    let mut doc = match crate::document::open_with_ctx(bytes, limits, ctx) {
        Ok(d) => d,
        Err(_) => return Ok(Fail),
    };
    if let Ok(page_ids) = crate::tools::rotate::collect_page_ids(doc.inner(), ctx) {
        for target in &params.targets {
            let page_num = match target {
                RedactionTarget::Page { page } => *page,
                RedactionTarget::Rectangle { page, .. } => *page,
                _ => continue,
            };
            if let Some(pid) = page_num
                .checked_sub(1)
                .and_then(|idx| page_ids.get(idx as usize))
                && let Ok(pdict) = doc.inner().get_dictionary(*pid)
                && pdict.has(b"Thumb")
            {
                return Ok(Fail);
            }
        }
    }
    if params.scope.attachments {
        if doc.inner().trailer.has(b"EmbeddedFiles") || doc.inner().trailer.has(b"AF") {
            return Ok(Fail);
        }
        if let Ok(catalog) = doc.inner().catalog()
            && (catalog.has(b"EmbeddedFiles")
                || catalog.has(b"AF")
                || catalog
                    .get(b"Names")
                    .and_then(Object::as_dict)
                    .is_ok_and(|n| n.has(b"EmbeddedFiles")))
        {
            return Ok(Fail);
        }
    }
    if !has_text_targets {
        return Ok(Pass);
    }
    let matchers = Matchers::new(&params.targets).map_err(|e| PdfError::InvalidParams(e.into()))?;
    doc.inner_mut()?.trailer.remove(b"ID");

    let mut budget = MatchBudget::with_context(ctx);
    let mut decode_budget = doc.decode_budget();
    let mut pending = VecDeque::new();
    let mut visited = HashSet::new();

    for (_, value) in doc.inner().trailer.iter() {
        enqueue_references(value, 0, &mut pending)?;
    }

    let mut visits = 0usize;
    while let Some((id, depth)) = pending.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        visits += 1;
        if visits > MAX_OBJECTS || depth > MAX_DEPTH {
            return Err(PdfError::Malformed);
        }
        if visits.is_multiple_of(128) {
            ctx.check()?;
        }
        let Some(obj) = doc.inner().objects.get(&id) else {
            continue;
        };
        if !check_object_for_targets(
            obj,
            &matchers,
            &params.targets,
            &mut budget,
            &mut decode_budget,
            &mut pending,
            depth,
            ctx,
        )? {
            return Ok(Fail);
        }
    }
    Ok(Pass)
}

fn enqueue_references(
    value: &Object,
    depth: usize,
    pending: &mut VecDeque<(ObjectId, usize)>,
) -> Result<(), PdfError> {
    match value {
        Object::Reference(id) => pending.push_back((*id, depth + 1)),
        Object::Array(items) => {
            for item in items {
                enqueue_references(item, depth + 1, pending)?;
            }
        }
        Object::Dictionary(dict) => {
            for (_, item) in dict.iter() {
                enqueue_references(item, depth + 1, pending)?;
            }
        }
        Object::Stream(stream) => {
            for (_, item) in stream.dict.iter() {
                enqueue_references(item, depth + 1, pending)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn check_object_for_targets(
    value: &Object,
    matchers: &Matchers,
    targets: &[ariad_core::tools::params::redact::RedactionTarget],
    budget: &mut MatchBudget,
    decode_budget: &mut crate::document::DecodeBudget,
    pending: &mut VecDeque<(ObjectId, usize)>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    use ariad_core::tools::params::redact::RedactionTarget;
    match value {
        Object::Reference(id) => {
            pending.push_back((*id, depth + 1));
        }
        Object::Array(items) => {
            for item in items {
                if !check_object_for_targets(
                    item,
                    matchers,
                    targets,
                    budget,
                    decode_budget,
                    pending,
                    depth + 1,
                    ctx,
                )? {
                    return Ok(false);
                }
            }
        }
        Object::Dictionary(dict) => {
            for (_, item) in dict.iter() {
                if !check_object_for_targets(
                    item,
                    matchers,
                    targets,
                    budget,
                    decode_budget,
                    pending,
                    depth + 1,
                    ctx,
                )? {
                    return Ok(false);
                }
            }
        }
        Object::String(bytes, _) => {
            if let Ok(text) = decode_string(bytes) {
                let found = matchers.find(&text, budget);
                ctx.check()?;
                if !found
                    .map_err(|e| PdfError::InvalidParams(e.into()))?
                    .is_empty()
                {
                    return Ok(false);
                }
            }
        }
        Object::Stream(stream) => {
            for (_, item) in stream.dict.iter() {
                if !check_object_for_targets(
                    item,
                    matchers,
                    targets,
                    budget,
                    decode_budget,
                    pending,
                    depth + 1,
                    ctx,
                )? {
                    return Ok(false);
                }
            }
            let is_image = stream
                .dict
                .get(b"Subtype")
                .is_ok_and(|s| matches!(s.as_name(), Ok(b"Image")));
            let is_font = stream.dict.get(b"Subtype").is_ok_and(|s| {
                matches!(s.as_name(), Ok(b"Type1C" | b"CIDFontType0C" | b"OpenType"))
            }) || stream.dict.has(b"FontFile")
                || stream.dict.has(b"FontFile2")
                || stream.dict.has(b"FontFile3");

            if !is_image && !is_font {
                let decoded = match decode_budget.decode(stream) {
                    Ok(Some(d)) => d,
                    _ => return Ok(false),
                };
                if let Ok(text) = std::str::from_utf8(&decoded) {
                    let found = matchers.find(text, budget);
                    ctx.check()?;
                    if !found
                        .map_err(|e| PdfError::InvalidParams(e.into()))?
                        .is_empty()
                    {
                        return Ok(false);
                    }
                }
                if decoded.len() >= 2 && decoded.len() % 2 == 0 {
                    let u16s_be: Vec<u16> = decoded
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&[c0, c1]| u16::from_be_bytes([c0, c1]))
                        .collect();
                    if let Ok(text_be) = String::from_utf16(&u16s_be) {
                        let found = matchers.find(&text_be, budget);
                        ctx.check()?;
                        if !found
                            .map_err(|e| PdfError::InvalidParams(e.into()))?
                            .is_empty()
                        {
                            return Ok(false);
                        }
                    }
                    let u16s_le: Vec<u16> = decoded
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&[c0, c1]| u16::from_le_bytes([c0, c1]))
                        .collect();
                    if let Ok(text_le) = String::from_utf16(&u16s_le) {
                        let found = matchers.find(&text_le, budget);
                        ctx.check()?;
                        if !found
                            .map_err(|e| PdfError::InvalidParams(e.into()))?
                            .is_empty()
                        {
                            return Ok(false);
                        }
                    }
                }
                for target in targets {
                    if let RedactionTarget::Exact { text } = target
                        && !text.is_empty()
                        && decoded.windows(text.len()).any(|w| w == text.as_bytes())
                    {
                        return Ok(false);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::{limits::Limits, tools::params::redact::RedactionTarget};
    use lopdf::{Document, Stream, dictionary};

    fn sample(bad_utf16: bool) -> PdfDoc {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let stream = doc.add_object(Stream::new(dictionary! {}, b"stream payload".to_vec()));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()], "Contents" => stream });
        doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages, "Title" => lopdf::text_string("secret ắ"), "Other" => Object::String(if bad_utf16 { vec![0xfe, 0xff, 0x00] } else { b"public".to_vec() }, StringFormat::Literal) });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        crate::document::open(&bytes, &Limits::local()).unwrap()
    }

    #[test]
    fn scrubs_utf16_strings_and_reports_unexamined_streams() {
        let mut pdf = sample(false);
        let matcher = Matchers::new(&[RedactionTarget::Exact {
            text: "a\u{306}\u{301}".into(),
        }])
        .unwrap();
        let stats = scrub_object_strings(
            &mut pdf,
            &matcher,
            &mut MatchBudget::default(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(stats.scrubbed, 1);
        assert_eq!(stats.uninspected_streams, 1);
        let catalog = pdf.inner().catalog().unwrap();
        assert!(catalog.get(b"Title").unwrap().as_str().unwrap().is_empty());
        assert_eq!(catalog.get(b"Other").unwrap().as_str().unwrap(), b"public");
    }

    #[test]
    fn malformed_strings_fail_without_mutating_the_document() {
        let mut pdf = sample(true);
        let before = pdf
            .inner()
            .catalog()
            .unwrap()
            .get(b"Title")
            .unwrap()
            .clone();
        let matcher = Matchers::new(&[RedactionTarget::Exact {
            text: "secret".into(),
        }])
        .unwrap();
        assert!(
            scrub_object_strings(
                &mut pdf,
                &matcher,
                &mut MatchBudget::default(),
                &ToolCtx::new()
            )
            .is_err()
        );
        assert_eq!(
            pdf.inner().catalog().unwrap().get(b"Title").unwrap(),
            &before
        );
    }

    #[test]
    fn removes_opaque_attachment_and_xfa_references_before_full_rewrite() {
        let mut pdf = sample(false);
        let doc = pdf.inner_mut().unwrap();
        let attachment = doc.add_object(Stream::new(
            dictionary! { "Type" => "EmbeddedFile" },
            b"attachment-secret-sentinel".to_vec(),
        ));
        let file_spec = doc.add_object(
            dictionary! { "Type" => "Filespec", "EF" => dictionary! { "F" => attachment } },
        );
        let xfa = doc.add_object(Stream::new(dictionary! {}, b"xfa-secret-sentinel".to_vec()));
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let catalog = doc.get_dictionary_mut(root).unwrap();
        catalog.set("AF", vec![Object::Reference(file_spec)]);
        catalog.set("Names", dictionary! { "EmbeddedFiles" => dictionary! { "Names" => vec![lopdf::text_string("example"), Object::Reference(file_spec)] } });
        catalog.set("AcroForm", dictionary! { "XFA" => xfa });
        assert!(remove_opaque_carriers(&mut pdf, &ToolCtx::new()).unwrap() >= 4);
        let bytes = crate::writer::save(&mut pdf, &ToolCtx::new()).unwrap();
        for sentinel in [
            b"attachment-secret-sentinel".as_slice(),
            b"xfa-secret-sentinel",
        ] {
            assert!(
                !bytes
                    .windows(sentinel.len())
                    .any(|window| window == sentinel)
            );
        }
        let reopened = crate::document::open(&bytes, &Limits::local()).unwrap();
        assert_eq!(
            reopened
                .inner()
                .catalog()
                .unwrap()
                .get(b"Title")
                .unwrap()
                .as_str()
                .unwrap(),
            lopdf::text_string("secret ắ").as_str().unwrap()
        );
    }
}
