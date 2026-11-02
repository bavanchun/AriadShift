//! Conservative Type0 font program subsetting.

mod truetype;
mod type1;

use ariad_core::limits::Limits;

use crate::{PdfDoc, PdfError, ToolCtx, document::DecodeBudget};
use lopdf::{Dictionary, Object, ObjectId, Stream, content::Content, dictionary};
use std::collections::{BTreeMap, BTreeSet};
use subsetter::GlyphRemapper;

/// Glyph IDs seen in identity-encoded Type0 fonts.
#[derive(Clone, Debug, Default)]
pub struct GlyphUsage {
    fonts: BTreeMap<ObjectId, BTreeSet<u16>>,
    simple: BTreeMap<ObjectId, BTreeSet<u8>>,
    incomplete: bool,
}

impl GlyphUsage {
    /// Returns the CIDs observed for a font; an ambiguous scan may be incomplete.
    #[must_use]
    pub fn glyphs_for(&self, font: ObjectId) -> Option<&BTreeSet<u16>> {
        self.fonts.get(&font)
    }
}

/// Result of attempting to subset an embedded font.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubsetOutcome {
    Subset { glyphs: usize },
    Skipped(&'static str),
}

/// Font parsing, PDF and cancellation errors.
#[derive(Debug, thiserror::Error)]
pub enum FontError {
    #[error(transparent)]
    Pdf(#[from] PdfError),
    #[error("invalid font program: {0}")]
    Program(String),
    #[error("malformed PDF content")]
    Malformed,
}

/// Subset a single face and return the program and the old-to-new glyph map.
pub fn subset_program(
    bytes: &[u8],
    glyphs: &[u16],
) -> Result<(Vec<u8>, BTreeMap<u16, u16>), FontError> {
    let distinct: BTreeSet<u16> = glyphs.iter().copied().chain(std::iter::once(0)).collect();
    if distinct.len() >= usize::from(u16::MAX) {
        return Err(FontError::Program("too many glyphs".into()));
    }
    let mapper = GlyphRemapper::new_from_glyphs_sorted(&distinct.into_iter().collect::<Vec<_>>());
    let out =
        subsetter::subset(bytes, 0, &mapper).map_err(|e| FontError::Program(e.to_string()))?;
    let map = mapper
        .remapped_gids()
        .filter_map(|old| mapper.get(old).map(|new| (old, new)))
        .collect();
    Ok((out, map))
}

fn resolve<'a>(doc: &'a lopdf::Document, mut value: &'a Object, limit: u16) -> Option<&'a Object> {
    for _ in 0..=limit {
        match value {
            Object::Reference(id) => value = doc.get_object(*id).ok()?,
            _ => return Some(value),
        }
    }
    None
}
fn dict<'a>(doc: &'a lopdf::Document, value: &'a Object, limit: u16) -> Option<&'a Dictionary> {
    resolve(doc, value, limit)?.as_dict().ok()
}
fn font_for(
    doc: &lopdf::Document,
    resources: &Dictionary,
    name: &[u8],
    limit: u16,
) -> Option<ObjectId> {
    resources
        .get(b"Font")
        .ok()
        .and_then(|v| dict(doc, v, limit))?
        .get(name)
        .ok()?
        .as_reference()
        .ok()
}
// A shared descriptor or program must stay intact for the other font dictionaries.
fn shared_reference(doc: &lopdf::Document, target: ObjectId, limit: u16) -> bool {
    let mut seen = 0_u8;
    let mut stack: Vec<(&Object, u16)> = doc.objects.values().map(|v| (v, 0)).collect();
    stack.extend(doc.trailer.iter().map(|(_, v)| (v, 0)));
    while let Some((object, depth)) = stack.pop() {
        match object {
            Object::Reference(id) if *id == target => {
                seen += 1;
                if seen > 1 {
                    return true;
                }
            }
            Object::Array(items) => {
                if depth >= limit {
                    return true;
                }
                stack.extend(items.iter().map(|v| (v, depth + 1)));
            }
            Object::Dictionary(items) => {
                if depth >= limit {
                    return true;
                }
                stack.extend(items.iter().map(|(_, v)| (v, depth + 1)));
            }
            Object::Stream(stream) => {
                if depth >= limit {
                    return true;
                }
                stack.extend(stream.dict.iter().map(|(_, v)| (v, depth + 1)));
            }
            _ => {}
        }
    }
    false
}

fn shared_font_program(doc: &lopdf::Document, descriptor: &Object, key: &[u8], limit: u16) -> bool {
    if descriptor
        .as_reference()
        .ok()
        .is_some_and(|id| shared_reference(doc, id, limit))
    {
        return true;
    }
    let Some(descriptor) = dict(doc, descriptor, limit) else {
        return true;
    };
    descriptor
        .get(key)
        .ok()
        .and_then(|v| v.as_reference().ok())
        .is_some_and(|id| shared_reference(doc, id, limit))
}

fn content_bytes(stream: &Stream, budget: &mut DecodeBudget) -> Result<Vec<u8>, FontError> {
    if stream.dict.has(b"Filter") {
        budget.decode(stream)?.ok_or(FontError::Malformed)
    } else {
        budget.charge(stream.content.len())?;
        Ok(stream.content.clone())
    }
}
fn text(doc: &lopdf::Document, usage: &mut GlyphUsage, font: Option<ObjectId>, bytes: &[u8]) {
    let Some(font) = font else {
        usage.incomplete = true;
        return;
    };
    let subtype = doc
        .get_object(font)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Subtype").ok())
        .and_then(|o| o.as_name().ok());
    if matches!(subtype, Some(b"TrueType" | b"Type1" | b"MMType1")) {
        usage
            .simple
            .entry(font)
            .or_default()
            .extend(bytes.iter().copied());
        return;
    }
    if subtype == Some(b"Type3") {
        return;
    }
    if subtype != Some(b"Type0") {
        usage.incomplete = true;
        return;
    }
    if !bytes.len().is_multiple_of(2) {
        usage.incomplete = true;
        return;
    }
    let glyphs = usage.fonts.entry(font).or_default();
    for chunk in bytes.as_chunks::<2>().0 {
        glyphs.insert(u16::from_be_bytes([chunk[0], chunk[1]]));
    }
}
fn scan(
    doc: &lopdf::Document,
    bytes: &[u8],
    resources: &Dictionary,
    usage: &mut GlyphUsage,
    ctx: &ToolCtx,
    limits: &Limits,
) -> Result<(), FontError> {
    ctx.check()?;
    let ops = match Content::decode(bytes) {
        Ok(ops) => ops,
        Err(_) => {
            usage.incomplete = true;
            return Ok(());
        }
    };
    let mut font = None;
    let mut stack = Vec::new();
    for op in ops.operations {
        match op.operator.as_str() {
            "q" => stack.push(font),
            "Q" => font = stack.pop().flatten(),
            "Tf" => {
                font = op
                    .operands
                    .first()
                    .and_then(|v| v.as_name().ok())
                    .and_then(|name| font_for(doc, resources, name, limits.max_nesting_depth));
                if font.is_none() {
                    usage.incomplete = true;
                }
            }
            "Tj" | "'" | "\"" => match op.operands.last() {
                Some(Object::String(bytes, _)) => text(doc, usage, font, bytes),
                _ => usage.incomplete = true,
            },
            "TJ" => match op.operands.first().and_then(|v| v.as_array().ok()) {
                Some(a) => {
                    for item in a {
                        match item {
                            Object::String(bytes, _) => text(doc, usage, font, bytes),
                            Object::Integer(_) | Object::Real(_) => {}
                            _ => usage.incomplete = true,
                        }
                    }
                }
                None => usage.incomplete = true,
            },
            _ => {}
        }
    }
    Ok(())
}

/// Scan page and resource streams; an ambiguous text operation makes subsetting unsafe.
pub fn glyph_usage(doc: &PdfDoc, ctx: &ToolCtx, limits: &Limits) -> Result<GlyphUsage, FontError> {
    let pdf = doc.inner();
    let mut budget = doc.decode_budget();
    let mut usage = GlyphUsage::default();
    for id in crate::color::page_ids(pdf, limits, ctx)? {
        ctx.check()?;
        let resources = crate::color::page_resources(pdf, id, limits)?;
        if let Ok(page) = pdf.get_dictionary(id)
            && let Ok(contents) = page.get(b"Contents")
        {
            let valid = match resolve(pdf, contents, limits.max_nesting_depth) {
                Some(Object::Stream(_)) => contents.as_reference().is_ok(),
                Some(Object::Array(items)) => items.iter().all(|item| item.as_reference().is_ok()),
                _ => false,
            };
            if !valid {
                usage.incomplete = true;
            }
        }
        let mut bytes = Vec::new();
        for content in pdf.get_page_contents(id) {
            let Ok(stream) = pdf.get_object(content).and_then(Object::as_stream) else {
                usage.incomplete = true;
                continue;
            };
            match content_bytes(stream, &mut budget) {
                Ok(decoded) => bytes.extend_from_slice(&decoded),
                Err(FontError::Malformed) => {
                    usage.incomplete = true;
                    continue;
                }
                Err(error) => return Err(error),
            }
            bytes.push(b'\n');
        }
        scan(pdf, &bytes, &resources, &mut usage, ctx, limits)?;
    }
    // Scan every form and tiling pattern, even if unused, so no reachable text is lost.
    for object in pdf.objects.values() {
        if let Ok(font) = object.as_dict()
            && font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type3".as_slice())
        {
            let resources = font
                .get(b"Resources")
                .ok()
                .and_then(|o| dict(pdf, o, limits.max_nesting_depth));
            let procedures = font
                .get(b"CharProcs")
                .ok()
                .and_then(|o| dict(pdf, o, limits.max_nesting_depth));
            match (resources, procedures) {
                (Some(resources), Some(procedures)) => {
                    for (_, proc) in procedures.iter() {
                        ctx.check()?;
                        let Some(stream) = resolve(pdf, proc, limits.max_nesting_depth)
                            .and_then(|o| o.as_stream().ok())
                        else {
                            usage.incomplete = true;
                            continue;
                        };
                        match content_bytes(stream, &mut budget) {
                            Ok(bytes) => scan(pdf, &bytes, resources, &mut usage, ctx, limits)?,
                            Err(FontError::Malformed) => usage.incomplete = true,
                            Err(error) => return Err(error),
                        }
                    }
                }
                _ => usage.incomplete = true,
            }
        }
        let Object::Stream(stream) = object else {
            continue;
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .unwrap_or_default();
        let pattern = stream
            .dict
            .get(b"PatternType")
            .and_then(Object::as_i64)
            .ok()
            == Some(1);
        if subtype != b"Form" && !pattern {
            continue;
        }
        ctx.check()?;
        let empty = Dictionary::new();
        let resources = stream
            .dict
            .get(b"Resources")
            .ok()
            .and_then(|o| dict(pdf, o, limits.max_nesting_depth))
            .unwrap_or(&empty);
        let bytes = match content_bytes(stream, &mut budget) {
            Ok(bytes) => bytes,
            Err(FontError::Malformed) => {
                usage.incomplete = true;
                continue;
            }
            Err(error) => return Err(error),
        };
        scan(pdf, &bytes, resources, &mut usage, ctx, limits)?;
    }
    Ok(usage)
}

/// Re-subset a TrueType Type0 font with an identity CID-to-GID mapping.
pub fn subset_type0(
    doc: &mut PdfDoc,
    font_id: ObjectId,
    usage: &GlyphUsage,
    limits: &Limits,
) -> Result<SubsetOutcome, FontError> {
    if usage.incomplete {
        return Ok(SubsetOutcome::Skipped("ambiguous glyph scan"));
    }
    let Some(glyphs) = usage.fonts.get(&font_id) else {
        return Ok(SubsetOutcome::Skipped("font not used"));
    };
    let pdf = doc.inner();
    let Some(font) = pdf.get_object(font_id).ok().and_then(|o| o.as_dict().ok()) else {
        return Ok(SubsetOutcome::Skipped("missing font"));
    };
    if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type0".as_slice()) {
        return Ok(SubsetOutcome::Skipped("font is not Type0"));
    }
    if !matches!(
        font.get(b"Encoding").and_then(Object::as_name),
        Ok(b"Identity-H" | b"Identity-V")
    ) {
        return Ok(SubsetOutcome::Skipped("non-identity encoding"));
    }
    let Some(descendants) = font
        .get(b"DescendantFonts")
        .ok()
        .and_then(|v| v.as_array().ok())
    else {
        return Ok(SubsetOutcome::Skipped("missing descendant"));
    };
    if descendants.len() != 1 {
        return Ok(SubsetOutcome::Skipped("multiple descendants"));
    }
    let Some(descendant) = dict(pdf, &descendants[0], limits.max_nesting_depth).cloned() else {
        return Ok(SubsetOutcome::Skipped("missing descendant"));
    };
    if descendant.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"CIDFontType2".as_slice())
    {
        return Ok(SubsetOutcome::Skipped("font is not CIDFontType2"));
    }
    if let Ok(map) = descendant.get(b"CIDToGIDMap")
        && map.as_name().ok() != Some(b"Identity".as_slice())
    {
        return Ok(SubsetOutcome::Skipped("non-identity CIDToGIDMap"));
    }
    if descendant
        .get(b"FontDescriptor")
        .ok()
        .is_some_and(|descriptor| {
            shared_font_program(pdf, descriptor, b"FontFile2", limits.max_nesting_depth)
        })
    {
        return Ok(SubsetOutcome::Skipped("shared font program"));
    }
    let Some(descriptor) = descendant
        .get(b"FontDescriptor")
        .ok()
        .and_then(|v| dict(pdf, v, limits.max_nesting_depth))
        .cloned()
    else {
        return Ok(SubsetOutcome::Skipped("missing font descriptor"));
    };
    let Some(program) = descriptor
        .get(b"FontFile2")
        .ok()
        .and_then(|v| resolve(pdf, v, limits.max_nesting_depth))
        .and_then(|v| v.as_stream().ok())
    else {
        return Ok(SubsetOutcome::Skipped("missing TrueType program"));
    };
    let mut budget = doc.decode_budget();
    let bytes = match content_bytes(program, &mut budget) {
        Ok(bytes) => bytes,
        Err(FontError::Malformed) => return Ok(SubsetOutcome::Skipped("unsupported font stream")),
        Err(error) => return Err(error),
    };
    let (subset, map) = match subset_program(&bytes, &glyphs.iter().copied().collect::<Vec<_>>()) {
        Ok(v) => v,
        Err(FontError::Program(_)) => {
            return Ok(SubsetOutcome::Skipped("unsupported font program"));
        }
        Err(e) => return Err(e),
    };
    let max = usize::from(*map.keys().max().unwrap_or(&0));
    let mut mapping = vec![0_u8; (max + 1) * 2];
    for (old, new) in map {
        mapping[usize::from(old) * 2..usize::from(old) * 2 + 2].copy_from_slice(&new.to_be_bytes());
    }
    let mut descriptor = descriptor;
    let mut descendant = descendant;
    let pdf = doc.inner_mut()?;
    let file_id = pdf.add_object(Stream::new(
        dictionary! {"Length1"=>subset.len() as i64},
        subset,
    ));
    descriptor.set("FontFile2", file_id);
    descriptor.remove(b"CIDSet");
    let descriptor_id = pdf.add_object(descriptor);
    let map_id = pdf.add_object(Stream::new(Dictionary::new(), mapping));
    descendant.set("FontDescriptor", descriptor_id);
    descendant.set("CIDToGIDMap", map_id);
    let descendant_id = pdf.add_object(descendant);
    pdf.get_object_mut(font_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| FontError::Malformed)?
        .set("DescendantFonts", vec![Object::Reference(descendant_id)]);
    Ok(SubsetOutcome::Subset {
        glyphs: glyphs.len(),
    })
}

/// Prune outlines from a simple TrueType font without changing character codes or GIDs.
pub fn subset_simple_truetype(
    doc: &mut PdfDoc,
    font_id: ObjectId,
    usage: &GlyphUsage,
    limits: &Limits,
) -> Result<SubsetOutcome, FontError> {
    if usage.incomplete {
        return Ok(SubsetOutcome::Skipped("ambiguous glyph scan"));
    }
    let Some(codes) = usage.simple.get(&font_id) else {
        return Ok(SubsetOutcome::Skipped("font not used"));
    };
    let pdf = doc.inner();
    let Some(font) = pdf.get_object(font_id).ok().and_then(|v| v.as_dict().ok()) else {
        return Ok(SubsetOutcome::Skipped("missing font"));
    };
    if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"TrueType") {
        return Ok(SubsetOutcome::Skipped("font is not simple TrueType"));
    }
    if !matches!(
        font.get(b"Encoding").and_then(Object::as_name),
        Ok(b"WinAnsiEncoding" | b"MacRomanEncoding")
    ) {
        return Ok(SubsetOutcome::Skipped("unsupported font encoding"));
    }
    if font.get(b"FontDescriptor").ok().is_some_and(|descriptor| {
        shared_font_program(pdf, descriptor, b"FontFile2", limits.max_nesting_depth)
    }) {
        return Ok(SubsetOutcome::Skipped("shared font program"));
    }
    let Some(mut descriptor) = font
        .get(b"FontDescriptor")
        .ok()
        .and_then(|v| dict(pdf, v, limits.max_nesting_depth))
        .cloned()
    else {
        return Ok(SubsetOutcome::Skipped("missing font descriptor"));
    };
    if descriptor
        .get(b"Flags")
        .and_then(Object::as_i64)
        .unwrap_or(0)
        & 32
        == 0
    {
        return Ok(SubsetOutcome::Skipped("symbolic or unspecified font"));
    }
    let Some(program) = descriptor
        .get(b"FontFile2")
        .ok()
        .and_then(|v| resolve(pdf, v, limits.max_nesting_depth))
        .and_then(|v| v.as_stream().ok())
    else {
        return Ok(SubsetOutcome::Skipped("missing TrueType program"));
    };
    let mut budget = doc.decode_budget();
    let bytes = match content_bytes(program, &mut budget) {
        Ok(bytes) => bytes,
        Err(FontError::Malformed) => return Ok(SubsetOutcome::Skipped("unsupported font stream")),
        Err(error) => return Err(error),
    };
    let Ok(encoding) = font.get_font_encoding(pdf) else {
        return Ok(SubsetOutcome::Skipped("unsupported font encoding"));
    };
    let mut glyphs = BTreeSet::new();
    for code in codes {
        let Ok(text) = encoding.bytes_to_string(&[*code]) else {
            return Ok(SubsetOutcome::Skipped("unmapped character"));
        };
        let mut chars = text.chars();
        let Some(ch) = chars.next() else {
            return Ok(SubsetOutcome::Skipped("unmapped character"));
        };
        if chars.next().is_some() || ch == '\u{fffd}' {
            return Ok(SubsetOutcome::Skipped("unmapped character"));
        }
        let Some(mapped) = truetype::glyphs_for_code(&bytes, ch, *code) else {
            return Ok(SubsetOutcome::Skipped("unsupported cmap"));
        };
        glyphs.extend(mapped);
    }
    let Some(subset) = truetype::subset(&bytes, &glyphs) else {
        return Ok(SubsetOutcome::Skipped("unsupported font program"));
    };
    if subset.len() >= bytes.len() {
        return Ok(SubsetOutcome::Skipped("no size gain"));
    }
    let pdf = doc.inner_mut()?;
    let file_id = pdf.add_object(Stream::new(
        dictionary! {"Length1" => subset.len() as i64},
        subset,
    ));
    descriptor.set("FontFile2", file_id);
    let descriptor_id = pdf.add_object(descriptor);
    pdf.get_object_mut(font_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| FontError::Malformed)?
        .set("FontDescriptor", descriptor_id);
    Ok(SubsetOutcome::Subset {
        glyphs: glyphs.len(),
    })
}

fn standard_glyph_name(code: u8) -> Option<Vec<u8>> {
    const DIGIT_NAMES: [&[u8]; 10] = [
        b"zero", b"one", b"two", b"three", b"four", b"five", b"six", b"seven", b"eight", b"nine",
    ];
    let name: &[u8] = match code {
        b'A'..=b'Z' | b'a'..=b'z' => return Some(vec![code]),
        b'0'..=b'9' => DIGIT_NAMES[usize::from(code - b'0')],
        b' ' => b"space",
        b'!' => b"exclam",
        b'"' => b"quotedbl",
        b'#' => b"numbersign",
        b'$' => b"dollar",
        b'%' => b"percent",
        b'&' => b"ampersand",
        b'\'' => b"quoteright",
        b'(' => b"parenleft",
        b')' => b"parenright",
        b'*' => b"asterisk",
        b'+' => b"plus",
        b',' => b"comma",
        b'-' => b"hyphen",
        b'.' => b"period",
        b'/' => b"slash",
        b':' => b"colon",
        b';' => b"semicolon",
        b'<' => b"less",
        b'=' => b"equal",
        b'>' => b"greater",
        b'?' => b"question",
        b'@' => b"at",
        b'[' => b"bracketleft",
        b'\\' => b"backslash",
        b']' => b"bracketright",
        b'^' => b"asciicircum",
        b'_' => b"underscore",
        b'`' => b"quoteleft",
        b'{' => b"braceleft",
        b'|' => b"bar",
        b'}' => b"braceright",
        b'~' => b"asciitilde",
        193 => b"grave",
        194 => b"acute",
        195 => b"circumflex",
        196 => b"tilde",
        197 => b"macron",
        198 => b"breve",
        199 => b"dotaccent",
        200 => b"dieresis",
        202 => b"ring",
        203 => b"cedilla",
        205 => b"hungarumlaut",
        206 => b"ogonek",
        207 => b"caron",
        _ => return None,
    };
    Some(name.to_vec())
}

/// Prune simple Type 1 charstrings without changing the PDF font dictionary encoding.
pub fn subset_simple_type1(
    doc: &mut PdfDoc,
    font_id: ObjectId,
    usage: &GlyphUsage,
    limits: &Limits,
) -> Result<SubsetOutcome, FontError> {
    if usage.incomplete {
        return Ok(SubsetOutcome::Skipped("ambiguous glyph scan"));
    }
    let Some(codes) = usage.simple.get(&font_id) else {
        return Ok(SubsetOutcome::Skipped("font not used"));
    };
    let pdf = doc.inner();
    let Some(font) = pdf.get_object(font_id).ok().and_then(|o| o.as_dict().ok()) else {
        return Ok(SubsetOutcome::Skipped("missing font"));
    };
    if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type1") {
        return Ok(SubsetOutcome::Skipped("font is not Type 1"));
    }
    if font.get(b"FontDescriptor").ok().is_some_and(|descriptor| {
        shared_font_program(pdf, descriptor, b"FontFile", limits.max_nesting_depth)
    }) {
        return Ok(SubsetOutcome::Skipped("shared font program"));
    }
    let Some(mut descriptor) = font
        .get(b"FontDescriptor")
        .ok()
        .and_then(|o| dict(pdf, o, limits.max_nesting_depth))
        .cloned()
    else {
        return Ok(SubsetOutcome::Skipped("missing font descriptor"));
    };
    let Some(program) = descriptor
        .get(b"FontFile")
        .ok()
        .and_then(|o| resolve(pdf, o, limits.max_nesting_depth))
        .and_then(|o| o.as_stream().ok())
    else {
        return Ok(SubsetOutcome::Skipped("missing Type 1 program"));
    };
    let length = |key: &[u8]| {
        program
            .dict
            .get(key)
            .and_then(Object::as_i64)
            .ok()
            .and_then(|v| usize::try_from(v).ok())
    };
    let (Some(length1), Some(length2), Some(length3)) =
        (length(b"Length1"), length(b"Length2"), length(b"Length3"))
    else {
        return Ok(SubsetOutcome::Skipped("invalid Type 1 lengths"));
    };
    let mut budget = doc.decode_budget();
    let bytes = match content_bytes(program, &mut budget) {
        Ok(bytes) => bytes,
        Err(FontError::Malformed) => return Ok(SubsetOutcome::Skipped("unsupported font stream")),
        Err(e) => return Err(e),
    };
    if length1
        .checked_add(length2)
        .and_then(|v| v.checked_add(length3))
        != Some(bytes.len())
    {
        return Ok(SubsetOutcome::Skipped("invalid Type 1 lengths"));
    }
    let mut mapping: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    let encoding = font
        .get(b"Encoding")
        .ok()
        .and_then(|o| resolve(pdf, o, limits.max_nesting_depth));
    let program_uses_standard_encoding = bytes.get(..length1).is_some_and(|head| {
        head.windows(b"/Encoding StandardEncoding def".len())
            .any(|window| window == b"/Encoding StandardEncoding def")
    });
    let standard = match encoding {
        Some(Object::Name(name)) => name == b"StandardEncoding",
        Some(Object::Dictionary(d)) => d
            .get(b"BaseEncoding")
            .and_then(Object::as_name)
            .map_or(program_uses_standard_encoding, |name| {
                name == b"StandardEncoding"
            }),
        None => program_uses_standard_encoding,
        _ => false,
    };
    if !standard {
        return Ok(SubsetOutcome::Skipped("unsupported Type 1 encoding"));
    }
    for code in codes {
        if let Some(name) = standard_glyph_name(*code) {
            mapping.insert(*code, name);
        }
    }
    if let Some(Object::Dictionary(d)) = encoding
        && let Ok(items) = d.get(b"Differences").and_then(Object::as_array)
    {
        let mut next = None;
        for item in items {
            match item {
                Object::Integer(n) => next = u8::try_from(*n).ok(),
                Object::Name(name) => {
                    let Some(code) = next else {
                        return Ok(SubsetOutcome::Skipped("invalid differences"));
                    };
                    mapping.insert(code, name.clone());
                    next = code.checked_add(1);
                }
                _ => return Ok(SubsetOutcome::Skipped("invalid differences")),
            }
        }
    }
    let Some(names) = codes
        .iter()
        .map(|code| mapping.get(code).cloned())
        .collect::<Option<BTreeSet<_>>>()
    else {
        return Ok(SubsetOutcome::Skipped("unmapped character"));
    };
    let Some((subset, new_length2)) = type1::subset(&bytes, length1, length2, &names) else {
        return Ok(SubsetOutcome::Skipped("unsupported Type 1 program"));
    };
    if subset.len() >= bytes.len() {
        return Ok(SubsetOutcome::Skipped("no size gain"));
    }
    let pdf = doc.inner_mut()?;
    let file_id = pdf.add_object(Stream::new(dictionary! {"Length1" => length1 as i64, "Length2" => new_length2 as i64, "Length3" => length3 as i64}, subset));
    descriptor.set("FontFile", file_id);
    descriptor.remove(b"CharSet");
    let descriptor_id = pdf.add_object(descriptor);
    pdf.get_object_mut(font_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| FontError::Malformed)?
        .set("FontDescriptor", descriptor_id);
    Ok(SubsetOutcome::Subset {
        glyphs: names.len(),
    })
}
