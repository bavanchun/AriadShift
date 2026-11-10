//! Typed OOXML style definitions and a bounded, memory-only reference document.
//! No user supplied XML or font bytes enter the generated package.
use crate::{
    ir::{CharacterStyle, Document, ParagraphAlignment, ParagraphStyle, StyleId, TableStyle},
    limits::Limits,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocxStyles {
    paragraphs: BTreeMap<String, ParagraphStyle>,
    characters: BTreeMap<String, CharacterStyle>,
    tables: BTreeMap<String, TableStyle>,
    character_bases: BTreeMap<String, String>,
    table_bases: BTreeMap<String, String>,
    headings: BTreeMap<String, u8>,
}
impl DocxStyles {
    pub(super) fn new(document: &Document) -> Self {
        Self {
            character_bases: BTreeMap::new(),
            table_bases: BTreeMap::new(),
            headings: BTreeMap::new(),
            paragraphs: document
                .styles
                .paragraph
                .iter()
                .map(|(id, p)| {
                    (
                        named("P", id),
                        document
                            .styles
                            .resolve_paragraph(Some(id), &ParagraphStyle::default(), 100)
                            .map(|mut resolved| {
                                resolved.based_on = p.based_on.clone();
                                resolved
                            })
                            .unwrap_or_else(|_| p.clone()),
                    )
                })
                .collect(),
            characters: document
                .styles
                .character
                .iter()
                .map(|(id, c)| (named("C", id), c.clone()))
                .collect(),
            tables: document
                .styles
                .table
                .iter()
                .map(|(id, t)| {
                    (
                        named("T", id),
                        document
                            .styles
                            .resolve_table(Some(id), &ParagraphStyle::default(), 100)
                            .unwrap_or_else(|_| t.clone()),
                    )
                })
                .collect(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty() && self.characters.is_empty() && self.tables.is_empty()
    }
    pub(super) fn paragraph(
        &mut self,
        style: Option<&StyleId>,
        direct: &ParagraphStyle,
        resolved: &ParagraphStyle,
    ) -> Option<String> {
        if direct.is_empty()
            && let Some(style) = style
        {
            return Some(named("P", style));
        }
        if resolved.is_empty() {
            return None;
        }
        let key = synthetic("P", &(style, direct, resolved));
        let mut value = resolved.clone();
        value.based_on = style.cloned();
        self.paragraphs.insert(key.clone(), value);
        Some(key)
    }
    pub(super) fn character(
        &mut self,
        resolved: &CharacterStyle,
        base: Option<&StyleId>,
    ) -> Option<String> {
        if resolved.is_empty() {
            return base.map(|id| named("C", id));
        }
        let key = synthetic("C", &(base, resolved));
        if let Some(base) = base {
            self.character_bases.insert(key.clone(), named("C", base));
        }
        self.characters.insert(key.clone(), resolved.clone());
        Some(key)
    }
    pub(super) fn table(
        &mut self,
        resolved: &TableStyle,
        base: Option<&StyleId>,
    ) -> Option<String> {
        if resolved.is_empty() {
            return base.map(|id| named("T", id));
        }
        let key = synthetic("T", &(base, resolved));
        if let Some(base) = base {
            self.table_bases.insert(key.clone(), named("T", base));
        }
        self.tables.insert(key.clone(), resolved.clone());
        Some(key)
    }
    pub(super) fn heading(&mut self, paragraph: &ParagraphStyle, level: u8) -> String {
        let key = synthetic("P", &("heading", level, paragraph));
        self.paragraphs.insert(key.clone(), paragraph.clone());
        self.headings.insert(key.clone(), level.clamp(1, 6));
        key
    }
    /// Adds typed style definitions to an engine-generated, original Pandoc DOCX.
    /// The caller generates the template inside its confined workspace.
    pub fn reference_doc(&self, template: &[u8], limits: &Limits) -> Result<Vec<u8>, String> {
        self.rewrite_package(template, limits, true)
    }
    /// Materializes effective character properties on generated runs. OOXML's
    /// bold/italic style properties toggle; direct properties set absolute values.
    pub fn finish_docx(&self, bytes: &[u8], limits: &Limits) -> Result<Vec<u8>, String> {
        self.rewrite_package(bytes, limits, false)
    }
    fn rewrite_package(
        &self,
        template: &[u8],
        limits: &Limits,
        reference: bool,
    ) -> Result<Vec<u8>, String> {
        limits.validate().map_err(|e| e.to_string())?;
        let mut input = zip::ZipArchive::new(Cursor::new(template)).map_err(|e| e.to_string())?;
        if input.len() as u64 > u64::from(limits.max_archive_entries) {
            return Err("reference document exceeds entry budget".into());
        }
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut total = 0u64;
        let mut found = false;
        for index in 0..input.len() {
            let mut entry = input.by_index(index).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            if name.starts_with('/')
                || name.contains(['\\', ':'])
                || name.split('/').any(|p| p == "..")
            {
                return Err("invalid reference document member".into());
            }
            if entry.is_dir() {
                continue;
            }
            let budget = limits.max_decompressed_bytes.saturating_sub(total);
            if entry.size() > budget {
                return Err("reference document exceeds byte budget".into());
            }
            let mut bytes = Vec::new();
            (&mut entry)
                .take(budget.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > budget {
                return Err("reference document exceeds byte budget".into());
            }
            total += bytes.len() as u64;
            if name == "word/styles.xml" {
                let mut xml = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                let end = xml.rfind("</w:styles>").ok_or("missing styles root")?;
                let mut definitions = if reference {
                    self.definitions()
                } else {
                    String::new()
                };
                // Pandoc's default Table style refers to this built-in base but
                // can omit its declaration, including when no custom styles exist.
                // Declare the empty inherited baseline in generated packages;
                // imported packages still undergo strict reference validation.
                if !xml.contains("w:styleId=\"TableNormal\"") {
                    definitions.push_str("<w:style w:type=\"table\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/></w:style>");
                }
                if total.saturating_add(definitions.len() as u64) > limits.max_decompressed_bytes {
                    return Err("generated styles exceed byte budget".into());
                }
                total += definitions.len() as u64;
                xml.insert_str(end, &definitions);
                bytes = xml.into_bytes();
                found = true;
            } else if !reference
                && name.starts_with("word/")
                && name.ends_with(".xml")
                && name != "word/styles.xml"
            {
                let patched = self.direct_run_properties(&bytes)?;
                let growth = patched.len().saturating_sub(bytes.len()) as u64;
                if total.saturating_add(growth) > limits.max_decompressed_bytes {
                    return Err("generated run properties exceed byte budget".into());
                }
                total += growth;
                bytes = patched;
            }
            output
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .map_err(|e| e.to_string())?;
            output.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        if reference && !found {
            return Err("reference document has no styles.xml".into());
        }
        output
            .finish()
            .map(|c| c.into_inner())
            .map_err(|e| e.to_string())
    }
    fn direct_run_properties(&self, xml: &[u8]) -> Result<Vec<u8>, String> {
        use quick_xml::{Reader, Writer, events::Event};
        let mut reader = Reader::from_reader(xml);
        let mut writer = Writer::new(Vec::new());
        loop {
            let event = reader.read_event().map_err(|e| e.to_string())?;
            if matches!(&event, Event::Start(e) if e.name().as_ref() == "w:rPr") {
                let mut properties = vec![event.into_owned()];
                let mut depth = 1usize;
                let mut style = None;
                while depth > 0 {
                    let event = reader.read_event().map_err(|e| e.to_string())?;
                    match &event {
                        Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "w:rStyle" => {
                            for attr in e.attributes() {
                                let attr = attr.map_err(|e| e.to_string())?;
                                if attr.key.as_ref() == "w:val" {
                                    style = Some(
                                        attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                            .map_err(|e| e.to_string())?
                                            .into_owned(),
                                    );
                                }
                            }
                        }
                        Event::Eof => return Err("unclosed run properties".into()),
                        _ => {}
                    }
                    match &event {
                        Event::Start(_) => depth += 1,
                        Event::End(_) => depth -= 1,
                        _ => {}
                    }
                    properties.push(event.into_owned());
                }
                if let Some(character) = style.as_ref().and_then(|id| self.characters.get(id)) {
                    let mut direct = String::new();
                    character_properties(&mut direct, character);
                    let mut direct_reader = Reader::from_str(&direct);
                    let mut direct_events = Vec::new();
                    let mut replaced = std::collections::BTreeSet::new();
                    loop {
                        let event = direct_reader.read_event().map_err(|e| e.to_string())?;
                        if matches!(event, Event::Eof) {
                            break;
                        }
                        if let Event::Empty(e) | Event::Start(e) = &event {
                            replaced.insert(e.name().as_ref().to_owned());
                        }
                        direct_events.push(event.into_owned());
                    }
                    let mut retained = Vec::new();
                    let mut skip = 0usize;
                    for event in &properties[1..properties.len() - 1] {
                        match event {
                            Event::Start(e) if skip > 0 || replaced.contains(e.name().as_ref()) => {
                                skip += 1;
                                continue;
                            }
                            Event::End(_) if skip > 0 => {
                                skip -= 1;
                                continue;
                            }
                            Event::Empty(e) if skip > 0 || replaced.contains(e.name().as_ref()) => {
                                continue;
                            }
                            _ if skip > 0 => continue,
                            _ => {}
                        }
                        retained.push(event.clone());
                    }
                    // Merge whole child subtrees so existing properties such as
                    // vertAlign stay in schema order without splitting nested XML.
                    let mut children: Vec<Vec<Event<'_>>> = Vec::new();
                    let mut depth = 0usize;
                    for event in retained.into_iter().chain(direct_events) {
                        if depth == 0 {
                            children.push(Vec::new());
                        }
                        match &event {
                            Event::Start(_) => depth += 1,
                            Event::End(_) => depth -= 1,
                            _ => {}
                        }
                        children.last_mut().expect("child started").push(event);
                    }
                    children.sort_by_key(|child| match &child[0] {
                        Event::Start(e) | Event::Empty(e) => run_property_order(e.name().as_ref()),
                        _ => usize::MAX,
                    });
                    writer
                        .write_event(properties[0].clone())
                        .map_err(|e| e.to_string())?;
                    for child in children {
                        for event in child {
                            writer.write_event(event).map_err(|e| e.to_string())?;
                        }
                    }
                    writer
                        .write_event(properties.last().expect("closed properties").clone())
                        .map_err(|e| e.to_string())?;
                } else {
                    for event in properties {
                        writer.write_event(event).map_err(|e| e.to_string())?;
                    }
                }
            } else {
                if matches!(event, Event::Eof) {
                    break;
                }
                writer.write_event(event).map_err(|e| e.to_string())?;
            }
        }
        Ok(writer.into_inner())
    }
    pub(super) fn warnings(&self) -> Vec<crate::warning::Warning> {
        use crate::warning::{Warning, WarningCode};
        let mut warnings = Vec::new();
        if self
            .characters
            .values()
            .any(|c| c.weight.is_some_and(|w| w != 400 && w != 700))
        {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                "DOCX supports normal/bold font weights; other numeric weights were approximated.",
            ));
        }
        if self
            .characters
            .values()
            .any(|c| c.size_pt.is_some_and(|s| (s * 2.0).fract() != 0.0))
            || self
                .paragraphs
                .values()
                .any(|p| p.size_pt.is_some_and(|s| (s * 2.0).fract() != 0.0))
        {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                "DOCX font sizes were rounded to half points.",
            ));
        }
        if self
            .characters
            .values()
            .any(|c| c.font.as_ref().is_some_and(|f| !f.fallback.is_empty()))
            || self
                .paragraphs
                .values()
                .any(|p| p.font.as_ref().is_some_and(|f| !f.fallback.is_empty()))
        {
            warnings.push(Warning::new(WarningCode::UnsupportedNode, "DOCX retains the primary font family; the document viewer selects fallback families."));
        }
        warnings
    }
    fn definitions(&self) -> String {
        let mut out = String::new();
        for (id, value) in &self.paragraphs {
            open_style(&mut out, "paragraph", id);
            if let Some(base) = &value.based_on {
                element(&mut out, "basedOn", &named("P", base));
            }
            if let Some(level) = self.headings.get(id) {
                element(&mut out, "basedOn", &format!("Heading{level}"));
            }
            out.push_str("<w:pPr>");
            paragraph_properties(&mut out, value);
            if let Some(level) = self.headings.get(id) {
                element(&mut out, "outlineLvl", &(level - 1).to_string());
            }
            out.push_str("</w:pPr><w:rPr>");
            character_properties(
                &mut out,
                &CharacterStyle {
                    font: value.font.clone(),
                    size_pt: value.size_pt,
                    color: value.color.clone(),
                    ..Default::default()
                },
            );
            out.push_str("</w:rPr></w:style>");
        }
        for (id, value) in &self.characters {
            open_style(&mut out, "character", id);
            if let Some(base) = self.character_bases.get(id) {
                element(&mut out, "basedOn", base);
            }
            out.push_str("<w:rPr>");
            character_properties(&mut out, value);
            out.push_str("</w:rPr></w:style>");
        }
        for (id, value) in &self.tables {
            open_style(&mut out, "table", id);
            if let Some(base) = self.table_bases.get(id) {
                element(&mut out, "basedOn", base);
            }
            out.push_str("<w:pPr>");
            paragraph_properties(&mut out, &value.paragraph);
            out.push_str("</w:pPr><w:rPr>");
            character_properties(&mut out, &value.character);
            out.push_str("</w:rPr>");
            if let Some(background) = &value.background {
                out.push_str("<w:tblPr>");
                shading(&mut out, background.as_str());
                out.push_str("</w:tblPr>");
            }
            out.push_str("</w:style>");
        }
        out
    }
}
fn named(kind: &str, id: &StyleId) -> String {
    format!("Ariad{kind}Style{}", id.as_str())
}
fn synthetic(kind: &str, value: &impl serde::Serialize) -> String {
    format!(
        "Ariad{kind}Direct{}",
        hex::encode(Sha256::digest(
            serde_json::to_vec(value).expect("validated style serializes")
        ))
    )
}
fn xml(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            *c == '\t'
                || *c == '\n'
                || *c == '\r'
                || (*c >= ' ' && !matches!(*c, '\u{fffe}' | '\u{ffff}'))
        })
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            c => c.to_string(),
        })
        .collect()
}
fn open_style(out: &mut String, kind: &str, id: &str) {
    out.push_str(&format!(
        "<w:style w:type=\"{kind}\" w:customStyle=\"1\" w:styleId=\"{}\"><w:name w:val=\"{}\"/>",
        xml(id),
        xml(id)
    ));
}
fn element(out: &mut String, name: &str, value: &str) {
    out.push_str(&format!("<w:{name} w:val=\"{}\"/>", xml(value)));
}
fn units(value: f64, factor: f64) -> String {
    (value * factor).round().to_string()
}
fn shading(out: &mut String, value: &str) {
    out.push_str(&format!(
        "<w:shd w:val=\"clear\" w:fill=\"{}\"/>",
        xml(value.trim_start_matches('#'))
    ));
}
fn paragraph_properties(out: &mut String, p: &ParagraphStyle) {
    if let Some(value) = p.keep_with_next {
        element(out, "keepNext", if value { "1" } else { "0" });
    }
    if let Some(value) = p.page_break_before {
        element(out, "pageBreakBefore", if value { "1" } else { "0" });
    }
    if let Some(value) = &p.background {
        shading(out, value.as_str());
    }
    if p.space_before_pt.is_some() || p.space_after_pt.is_some() || p.line_height.is_some() {
        out.push_str("<w:spacing");
        for (name, value, factor) in [
            ("before", p.space_before_pt, 20.0),
            ("after", p.space_after_pt, 20.0),
            ("line", p.line_height, 240.0),
        ] {
            if let Some(value) = value {
                out.push_str(&format!(" w:{name}=\"{}\"", units(value, factor)));
            }
        }
        if p.line_height.is_some() {
            out.push_str(" w:lineRule=\"auto\"");
        }
        out.push_str("/>");
    }
    if p.indent_left_pt.is_some() || p.indent_right_pt.is_some() || p.indent_first_pt.is_some() {
        out.push_str("<w:ind");
        for (name, value) in [("left", p.indent_left_pt), ("right", p.indent_right_pt)] {
            if let Some(value) = value {
                out.push_str(&format!(" w:{name}=\"{}\"", units(value, 20.0)));
            }
        }
        if let Some(value) = p.indent_first_pt {
            out.push_str(&format!(
                " w:{}=\"{}\"",
                if value < 0.0 { "hanging" } else { "firstLine" },
                units(value.abs(), 20.0)
            ));
        }
        out.push_str("/>");
    }
    if let Some(value) = p.align {
        element(
            out,
            "jc",
            match value {
                ParagraphAlignment::Left => "left",
                ParagraphAlignment::Center => "center",
                ParagraphAlignment::Right => "right",
                ParagraphAlignment::Justify => "both",
            },
        );
    }
}
fn run_property_order(name: &str) -> usize {
    // WordprocessingML run-property order, including properties retained from Pandoc.
    const ORDER: &[&str] = &[
        "rStyle",
        "rFonts",
        "b",
        "bCs",
        "i",
        "iCs",
        "caps",
        "smallCaps",
        "strike",
        "dstrike",
        "outline",
        "shadow",
        "emboss",
        "imprint",
        "noProof",
        "snapToGrid",
        "vanish",
        "webHidden",
        "color",
        "spacing",
        "w",
        "kern",
        "position",
        "sz",
        "szCs",
        "highlight",
        "u",
        "effect",
        "bdr",
        "shd",
        "fitText",
        "vertAlign",
        "rtl",
        "cs",
        "em",
        "lang",
        "eastAsianLayout",
        "specVanish",
        "oMath",
        "rPrChange",
    ];
    let name = name.strip_prefix("w:").unwrap_or(name);
    ORDER
        .iter()
        .position(|property| *property == name)
        .unwrap_or(ORDER.len())
}
fn character_properties(out: &mut String, c: &CharacterStyle) {
    if let Some(font) = &c.font {
        let family = xml(&font.family);
        out.push_str(&format!("<w:rFonts w:ascii=\"{family}\" w:hAnsi=\"{family}\" w:eastAsia=\"{family}\" w:cs=\"{family}\"/>"));
    }
    if let Some(value) = c.weight {
        element(out, "b", if value >= 600 { "1" } else { "0" });
        element(out, "bCs", if value >= 600 { "1" } else { "0" });
    }
    for (name, value) in [
        ("i", c.italic),
        ("smallCaps", c.small_caps),
        ("strike", c.strike),
    ] {
        if let Some(value) = value {
            element(out, name, if value { "1" } else { "0" });
        }
    }
    if let Some(value) = &c.color {
        element(out, "color", value.as_str().trim_start_matches('#'));
    }
    if let Some(value) = c.letter_spacing {
        element(out, "spacing", &units(value, 20.0));
    }
    if let Some(value) = c.baseline_shift {
        element(out, "position", &units(value, 2.0));
    }
    if let Some(value) = c.size_pt {
        element(out, "sz", &units(value, 2.0));
        element(out, "szCs", &units(value, 2.0));
    }
    if let Some(value) = c.underline {
        element(out, "u", if value { "single" } else { "none" });
    }
    if let Some(value) = &c.highlight {
        shading(out, value.as_str());
    }
}
