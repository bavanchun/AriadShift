//! Bounded, memory-only WordprocessingML style recovery.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};

use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
};
use sha2::{Digest, Sha256};

use crate::{
    ir::{
        CharacterStyle, Colour, FontRef, ParagraphAlignment, ParagraphStyle, StyleId, StyleSheet,
        TableStyle,
    },
    limits::Limits,
    warning::{Warning, WarningCode},
};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const STRICT_WORD_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const MAX_STYLES_BYTES: u64 = 64 * 1024 * 1024;

/// Legal names remain readable. Other names use the same deterministic identity in
/// both the package reader and Pandoc mapper; no lossy slug conversion is used.
pub(super) fn style_id(name: &str) -> StyleId {
    StyleId::new(name).unwrap_or_else(|_| {
        StyleId::new(format!("s{}", hex::encode(Sha256::digest(name.as_bytes()))))
            .expect("hash is a valid style identity")
    })
}

/// Run properties on a paragraph use a separate identity from a genuine named
/// character style, even when Word permits the same display name for both.
pub(super) fn paragraph_run_id(paragraph: &StyleId) -> StyleId {
    let mut hash = Sha256::new();
    hash.update(b"ariad-docx-paragraph-runs\0");
    hash.update(paragraph.as_str().as_bytes());
    StyleId::new(format!("pRun{}", hex::encode(hash.finalize())))
        .expect("hash is a valid style identity")
}

/// Recovers definitions from `word/styles.xml` without accessing the filesystem.
/// Missing styles yield an empty sheet and a warning. Invalid packages, XML,
/// duplicate identities/names, dangling inheritance and cycles return an error.
pub fn read_docx_styles(
    bytes: &[u8],
    limits: &Limits,
) -> Result<(StyleSheet, Vec<Warning>), String> {
    limits.validate().map_err(|e| e.to_string())?;
    if limits
        .max_input_bytes
        .is_some_and(|cap| bytes.len() as u64 > cap)
    {
        return Err("DOCX exceeds input byte limit".into());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > limits.max_archive_entries as usize {
        return Err("DOCX exceeds archive entry limit".into());
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    let mut xml = None;
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = member.name().map_err(|e| e.to_string())?.into_owned();
        if name.starts_with('/')
            || name.contains(['\\', ':'])
            || name.split('/').any(|part| part == ".." || part == ".")
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err("invalid or duplicate DOCX archive path".into());
        }
        total = total
            .checked_add(member.size())
            .ok_or("DOCX expanded size overflow")?;
        if total > limits.max_decompressed_bytes {
            return Err("DOCX exceeds decompressed byte limit".into());
        }
        if name == "word/styles.xml" {
            let cap = limits
                .max_ir_json_bytes
                .min(limits.max_decompressed_bytes)
                .min(MAX_STYLES_BYTES);
            if member.size() > cap {
                return Err("DOCX styles XML exceeds byte limit".into());
            }
            let mut data = Vec::new();
            (&mut member)
                .take(cap.saturating_add(1))
                .read_to_end(&mut data)
                .map_err(|e| e.to_string())?;
            if data.len() as u64 > cap {
                return Err("DOCX styles XML exceeds byte limit".into());
            }
            xml = Some(data);
        }
    }
    let Some(xml) = xml else {
        return Ok((
            StyleSheet::default(),
            vec![Warning::new(
                WarningCode::UnsupportedNode,
                "DOCX has no word/styles.xml definitions",
            )],
        ));
    };
    parse_styles(&xml, limits)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Paragraph,
    Character,
    Table,
    Unsupported,
}

#[derive(Clone, Debug)]
struct Definition {
    name: String,
    kind: Kind,
    based_on: Option<String>,
    paragraph: ParagraphStyle,
    character: CharacterStyle,
    background: Option<Colour>,
    is_default: bool,
}

#[derive(Default)]
pub(super) struct Notices(BTreeSet<String>);
impl Notices {
    fn ignored(&mut self, property: &str) {
        self.0.insert(format!(
            "DOCX style property `{property}` was not represented"
        ));
    }
    pub(super) fn warnings(self) -> Vec<Warning> {
        self.0
            .into_iter()
            .map(|msg| Warning::new(WarningCode::UnsupportedNode, msg))
            .collect()
    }
}

pub(super) fn is_word(ns: &ResolveResult<'_>) -> bool {
    matches!(ns, ResolveResult::Bound(ns) if ns.as_ref() == WORD_NS || ns.as_ref() == STRICT_WORD_NS)
}

pub(super) fn attributes(
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        let (ns, local) = reader.resolver().resolve_attribute(attr.key);
        if is_word(&ns) {
            let key = local.as_ref().to_owned();
            let value = attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|e| e.to_string())?
                .into_owned();
            if result.insert(key, value).is_some() {
                return Err("duplicate XML style attribute".into());
            }
        }
    }
    Ok(result)
}

fn parse_styles(xml: &[u8], limits: &Limits) -> Result<(StyleSheet, Vec<Warning>), String> {
    let mut reader = NsReader::from_reader(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut definitions = BTreeMap::<String, Definition>::new();
    let mut current: Option<(String, Definition)> = None;
    let mut sheet = StyleSheet::default();
    let mut notices = Notices::default();
    let mut root_seen = false;
    let mut root_closed = false;
    let mut nodes = 0u64;
    let node_cap = 8192u64.saturating_add(u64::from(limits.max_blocks).saturating_mul(64));
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(|e| e.to_string())?;
        let word = is_word(&ns);
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                nodes += 1;
                if nodes > node_cap {
                    return Err("DOCX styles XML exceeds node limit".into());
                }
                if stack.len() >= usize::from(limits.max_nesting_depth) {
                    return Err("DOCX styles XML exceeds depth limit".into());
                }
                let local = if word {
                    element.local_name().as_ref().to_owned()
                } else {
                    String::new()
                };
                if stack.is_empty() {
                    if root_seen || root_closed || local != "styles" {
                        return Err("invalid DOCX styles XML root".into());
                    }
                    root_seen = true;
                }
                let attrs = attributes(&reader, element)?;
                let parent = stack.last().map(String::as_str).unwrap_or("");
                if local == "style" && parent == "styles" {
                    if current.is_some() {
                        return Err("nested DOCX style definition".into());
                    }
                    let raw_id = attrs
                        .get("styleId")
                        .filter(|v| !v.is_empty())
                        .ok_or("DOCX style has no identity")?
                        .clone();
                    let kind = match attrs.get("type").map(String::as_str) {
                        Some("paragraph") => Kind::Paragraph,
                        Some("character") => Kind::Character,
                        Some("table") => Kind::Table,
                        _ => {
                            notices.ignored("style type");
                            Kind::Unsupported
                        }
                    };
                    let def = Definition {
                        name: raw_id.clone(),
                        kind,
                        based_on: None,
                        paragraph: ParagraphStyle::default(),
                        character: CharacterStyle::default(),
                        background: None,
                        is_default: attrs
                            .get("default")
                            .is_some_and(|v| v == "1" || v == "true" || v == "on"),
                    };
                    current = Some((raw_id, def));
                } else if let Some((_, def)) = &mut current {
                    if parent == "style" && local == "name" {
                        def.name = attrs
                            .get("val")
                            .filter(|v| !v.is_empty())
                            .ok_or("empty DOCX style name")?
                            .clone();
                    } else if parent == "style" && local == "basedOn" {
                        def.based_on = Some(
                            attrs
                                .get("val")
                                .filter(|v| !v.is_empty())
                                .ok_or("empty DOCX basedOn")?
                                .clone(),
                        );
                    } else if def.kind != Kind::Unsupported
                        && stack.len() == 3
                        && parent == "pPr"
                        && stack[1] == "style"
                    {
                        paragraph_property(&local, &attrs, &mut def.paragraph, &mut notices)?;
                    } else if def.kind != Kind::Unsupported
                        && stack.len() == 3
                        && parent == "rPr"
                        && stack[1] == "style"
                    {
                        character_property(&local, &attrs, &mut def.character, &mut notices)?;
                    } else if stack.len() == 3 && parent == "tblPr" && local == "shd" {
                        def.background = colour_attr(&attrs, "fill", &mut notices);
                    } else if parent == "style" && local == "tblStylePr" {
                        notices.ignored("conditional table formatting");
                    }
                } else if stack == ["styles", "docDefaults", "pPrDefault", "pPr"] {
                    paragraph_property(
                        &local,
                        &attrs,
                        &mut sheet.defaults.paragraph,
                        &mut notices,
                    )?;
                } else if stack == ["styles", "docDefaults", "rPrDefault", "rPr"] {
                    character_property(
                        &local,
                        &attrs,
                        &mut sheet.defaults.character,
                        &mut notices,
                    )?;
                }
                if matches!(event, Event::Start(_)) {
                    stack.push(local);
                } else if local == "style" && parent == "styles" {
                    insert_definition(&mut definitions, current.take(), limits)?;
                } else if stack.is_empty() {
                    root_closed = true;
                }
            }
            Event::End(_) => {
                let name = stack.pop().ok_or("unexpected XML end")?;
                if name == "style" && stack == ["styles"] {
                    insert_definition(&mut definitions, current.take(), limits)?;
                }
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            Event::DocType(_) | Event::GeneralRef(_) => {
                return Err("XML entities and DTDs are forbidden in DOCX styles".into());
            }
            Event::Text(text) => {
                if !text.as_ref().bytes().all(|b| b.is_ascii_whitespace()) {
                    return Err("unexpected text in DOCX styles XML".into());
                }
            }
            Event::CData(_) => return Err("unexpected CDATA in DOCX styles XML".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || !root_closed || !stack.is_empty() || current.is_some() {
        return Err("incomplete DOCX styles XML".into());
    }
    sheet
        .defaults
        .paragraph
        .overlay(&paragraph_font(&sheet.defaults.character));
    resolve_definitions(definitions, &mut sheet, limits)?;
    let byte_cap = limits
        .max_ir_json_bytes
        .min(limits.max_decompressed_bytes)
        .min(MAX_STYLES_BYTES);
    if serde_json::to_vec(&sheet).map_err(|e| e.to_string())?.len() as u64 > byte_cap {
        return Err("resolved DOCX styles exceed byte limit".into());
    }
    sheet.validate(limits.max_nesting_depth, limits.max_blocks as usize)?;
    Ok((sheet, notices.warnings()))
}

fn insert_definition(
    definitions: &mut BTreeMap<String, Definition>,
    current: Option<(String, Definition)>,
    limits: &Limits,
) -> Result<(), String> {
    let (id, def) = current.ok_or("missing DOCX style definition")?;
    if definitions.insert(id, def).is_some() {
        return Err("duplicate DOCX style identity".into());
    }
    if definitions.len() > limits.max_blocks as usize {
        return Err("DOCX style count exceeds block limit".into());
    }
    Ok(())
}

fn resolve_definitions(
    definitions: BTreeMap<String, Definition>,
    sheet: &mut StyleSheet,
    limits: &Limits,
) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut defaults = BTreeSet::new();
    let mut resolved_bytes = 0u64;
    let byte_cap = limits
        .max_ir_json_bytes
        .min(limits.max_decompressed_bytes)
        .min(MAX_STYLES_BYTES);
    for def in definitions.values() {
        let id = style_id(&def.name);
        if !names.insert((def.kind as u8, id)) {
            return Err("duplicate DOCX style name".into());
        }
    }
    for (raw_id, def) in &definitions {
        if def.kind == Kind::Unsupported {
            continue;
        }
        let id = style_id(&def.name);
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = Some(raw_id);
        while let Some(raw) = next {
            if chain.len() >= usize::from(limits.max_nesting_depth) {
                return Err("DOCX style inheritance exceeds depth limit".into());
            }
            if !seen.insert(raw) {
                return Err("DOCX style inheritance cycle".into());
            }
            let parent = definitions
                .get(raw)
                .ok_or("dangling DOCX basedOn reference")?;
            if parent.kind != def.kind {
                return Err("DOCX basedOn has a different style type".into());
            }
            chain.push(parent);
            next = parent.based_on.as_ref();
        }
        let mut paragraph = ParagraphStyle::default();
        let mut character = CharacterStyle::default();
        let mut background = None;
        for value in chain.into_iter().rev() {
            paragraph.overlay(&value.paragraph);
            paragraph.overlay(&paragraph_font(&value.character));
            character.overlay(&value.character);
            if value.background.is_some() {
                background.clone_from(&value.background);
            }
        }
        // Inheritance may repeat a long font family in thousands of effective
        // definitions. Charge expanded values before retaining those copies.
        resolved_bytes = resolved_bytes.saturating_add(
            serde_json::to_vec(&(&paragraph, &character, &background))
                .map_err(|e| e.to_string())?
                .len() as u64,
        );
        if resolved_bytes > byte_cap {
            return Err("resolved DOCX styles exceed byte limit".into());
        }
        if def.is_default && !defaults.insert(def.kind as u8) {
            return Err("duplicate default DOCX style".into());
        }
        match def.kind {
            Kind::Paragraph => {
                let mut named = def.paragraph.clone();
                named.overlay(&paragraph_font(&def.character));
                named.based_on = def
                    .based_on
                    .as_ref()
                    .map(|raw| style_id(&definitions[raw].name));
                // Paragraph rPr belongs to its text runs. Retain it as a companion
                // character definition because ParagraphStyle has no weight flags.
                if !character.is_empty() {
                    let run_id = paragraph_run_id(&id);
                    if names.contains(&(Kind::Character as u8, run_id.clone())) {
                        return Err("DOCX paragraph/run style identity collision".into());
                    }
                    sheet.character.insert(run_id, character.clone());
                }
                sheet.paragraph.insert(id, named);
                if def.is_default {
                    sheet.defaults.paragraph.overlay(&paragraph);
                    sheet.defaults.character.overlay(&character);
                }
            }
            Kind::Character => {
                if sheet.character.insert(id, character.clone()).is_some() {
                    return Err("DOCX paragraph/run style name collision".into());
                }
                if def.is_default {
                    sheet.defaults.character.overlay(&character);
                }
            }
            Kind::Table => {
                let value = TableStyle {
                    paragraph,
                    character,
                    background,
                };
                if def.is_default {
                    sheet.defaults.table = value.clone();
                }
                sheet.table.insert(id, value);
            }
            Kind::Unsupported => {}
        }
    }
    Ok(())
}

pub(super) fn paragraph_font(character: &CharacterStyle) -> ParagraphStyle {
    ParagraphStyle {
        font: character.font.clone(),
        size_pt: character.size_pt,
        color: character.color.clone(),
        ..ParagraphStyle::default()
    }
}

fn value(attrs: &BTreeMap<String, String>) -> Result<&str, String> {
    attrs
        .get("val")
        .map(String::as_str)
        .ok_or_else(|| "missing DOCX style property value".into())
}
fn number(text: &str) -> Result<f64, String> {
    let number: f64 = text
        .parse()
        .map_err(|_| "invalid DOCX numeric style value")?;
    if number.is_finite() {
        Ok(number)
    } else {
        Err("nonfinite DOCX style value".into())
    }
}
fn toggle(attrs: &BTreeMap<String, String>) -> Result<bool, String> {
    match attrs.get("val").map(String::as_str).unwrap_or("1") {
        "true" | "1" | "on" => Ok(true),
        "false" | "0" | "off" => Ok(false),
        _ => Err("invalid DOCX boolean style value".into()),
    }
}
fn colour_attr(
    attrs: &BTreeMap<String, String>,
    key: &str,
    notices: &mut Notices,
) -> Option<Colour> {
    let raw = attrs.get(key)?;
    match Colour::new(format!("#{raw}")) {
        Ok(colour) => Some(colour),
        Err(_) => {
            notices.ignored("automatic/theme colour");
            None
        }
    }
}
fn font(attrs: &BTreeMap<String, String>, notices: &mut Notices) -> Option<FontRef> {
    let mut families = Vec::new();
    for key in ["ascii", "hAnsi", "eastAsia", "cs"] {
        if let Some(name) = attrs.get(key).filter(|s| !s.trim().is_empty())
            && !families.contains(name)
        {
            families.push(name.clone());
        }
    }
    if families.is_empty() {
        notices.ignored("theme font");
        return None;
    }
    Some(FontRef {
        family: families.remove(0),
        fallback: families,
    })
}

pub(super) fn paragraph_property(
    name: &str,
    attrs: &BTreeMap<String, String>,
    style: &mut ParagraphStyle,
    notices: &mut Notices,
) -> Result<(), String> {
    match name {
        "jc" => {
            style.align = match value(attrs)? {
                "left" | "start" => Some(ParagraphAlignment::Left),
                "right" | "end" => Some(ParagraphAlignment::Right),
                "center" => Some(ParagraphAlignment::Center),
                "both" | "distribute" => Some(ParagraphAlignment::Justify),
                _ => {
                    notices.ignored("alignment");
                    None
                }
            }
        }
        "spacing" => {
            if let Some(raw) = attrs.get("before") {
                style.space_before_pt = Some(number(raw)? / 20.0);
            }
            if let Some(raw) = attrs.get("after") {
                style.space_after_pt = Some(number(raw)? / 20.0);
            }
            if let Some(raw) = attrs.get("line") {
                if attrs.get("lineRule").is_none_or(|v| v == "auto") {
                    style.line_height = Some(number(raw)? / 240.0);
                } else {
                    notices.ignored("absolute line spacing");
                }
            }
        }
        "ind" => {
            if let Some(raw) = attrs.get("firstLine") {
                style.indent_first_pt = Some(number(raw)? / 20.0);
            }
            if let Some(raw) = attrs.get("hanging") {
                style.indent_first_pt = Some(-number(raw)? / 20.0);
            }
            if let Some(raw) = attrs.get("start").or_else(|| attrs.get("left")) {
                style.indent_left_pt = Some(number(raw)? / 20.0);
            }
            if let Some(raw) = attrs.get("end").or_else(|| attrs.get("right")) {
                style.indent_right_pt = Some(number(raw)? / 20.0);
            }
        }
        "keepNext" => style.keep_with_next = Some(toggle(attrs)?),
        "pageBreakBefore" => style.page_break_before = Some(toggle(attrs)?),
        "shd" => style.background = colour_attr(attrs, "fill", notices),
        _ => notices.ignored(name),
    }
    style.validate()
}

pub(super) fn character_property(
    name: &str,
    attrs: &BTreeMap<String, String>,
    style: &mut CharacterStyle,
    notices: &mut Notices,
) -> Result<(), String> {
    match name {
        "rFonts" => style.font = font(attrs, notices),
        "sz" => style.size_pt = Some(number(value(attrs)?)? / 2.0),
        "b" => style.weight = Some(if toggle(attrs)? { 700 } else { 400 }),
        "i" => style.italic = Some(toggle(attrs)?),
        "u" => style.underline = Some(attrs.get("val").is_none_or(|v| v != "none")),
        "strike" | "dstrike" => style.strike = Some(toggle(attrs)?),
        "smallCaps" => style.small_caps = Some(toggle(attrs)?),
        "color" => style.color = colour_attr(attrs, "val", notices),
        "shd" => style.highlight = colour_attr(attrs, "fill", notices),
        "highlight" => {
            let colour = match value(attrs)? {
                "black" => "#000000",
                "blue" => "#0000ff",
                "cyan" => "#00ffff",
                "green" => "#00ff00",
                "magenta" => "#ff00ff",
                "red" => "#ff0000",
                "yellow" => "#ffff00",
                "white" => "#ffffff",
                "darkBlue" => "#000080",
                "darkCyan" => "#008080",
                "darkGreen" => "#008000",
                "darkMagenta" => "#800080",
                "darkRed" => "#800000",
                "darkYellow" => "#808000",
                "darkGray" => "#808080",
                "lightGray" => "#c0c0c0",
                _ => {
                    notices.ignored("highlight reset/colour");
                    return Ok(());
                }
            };
            style.highlight = Some(Colour::new(colour)?);
        }
        "position" => style.baseline_shift = Some(number(value(attrs)?)? / 2.0),
        "spacing" => style.letter_spacing = Some(number(value(attrs)?)? / 20.0),
        "vertAlign" if value(attrs)? == "baseline" => style.baseline_shift = Some(0.0),
        _ => notices.ignored(name),
    }
    style.validate()
}
