//! Recover document-local OOXML formatting omitted by Pandoc's semantic AST.
use super::docx_style_reader::{
    Notices, attributes, character_property, is_word, paragraph_font, paragraph_property,
    paragraph_run_id, style_id,
};
use crate::{
    ir::{Block, CharacterStyle, Document, Inline, ParagraphStyle, StyleId},
    limits::Limits,
    warning::{Warning, WarningCode},
};
use quick_xml::{events::Event, reader::NsReader};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Cursor, Read},
};
use unicode_normalization::UnicodeNormalization;

#[derive(Default)]
struct Run {
    text: String,
    style: Option<StyleId>,
    direct: CharacterStyle,
}
#[derive(Default)]
struct Paragraph {
    text: String,
    style: Option<StyleId>,
    direct: ParagraphStyle,
    character: CharacterStyle,
    runs: Vec<Run>,
}
#[derive(Default)]
struct Table {
    text: String,
    style: Option<StyleId>,
}

fn finish_run(paragraph: &mut Option<Paragraph>, run: &mut Option<Run>) {
    if let (Some(paragraph), Some(mut run)) = (paragraph, run.take()) {
        run.text = run.text.nfc().collect();
        paragraph.text.push_str(&run.text);
        paragraph.runs.push(run);
    }
}

fn finish_paragraph(
    paragraph: &mut Option<Paragraph>,
    paragraphs: &mut Vec<Paragraph>,
    tables: &mut [Table],
    active_tables: &[usize],
    limits: &Limits,
) -> Result<(), String> {
    let Some(mut paragraph) = paragraph.take() else {
        return Ok(());
    };
    paragraph
        .direct
        .overlay(&paragraph_font(&paragraph.character));
    for index in active_tables {
        tables[*index].text.push_str(&paragraph.text);
        tables[*index].text.push('\n');
    }
    paragraphs.push(paragraph);
    if paragraphs.len() > limits.max_blocks as usize {
        return Err("DOCX paragraph count exceeds block limit".into());
    }
    Ok(())
}

// Called only after read_docx_styles has checked every package path and expanded size.
fn member(bytes: &[u8], name: &str, limits: &Limits) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut member = match archive.by_name(name) {
        Ok(member) => member,
        Err(zip::result::ZipError::FileNotFound) => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let cap = limits.max_decompressed_bytes.min(limits.max_ir_json_bytes);
    if member.size() > cap {
        return Err("DOCX document XML exceeds byte limit".into());
    }
    let mut data = Vec::new();
    (&mut member)
        .take(cap.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > cap {
        return Err("DOCX document XML exceeds byte limit".into());
    }
    Ok(data)
}

fn names(xml: &[u8]) -> Result<BTreeMap<String, StyleId>, String> {
    let mut reader = NsReader::from_reader(xml);
    let mut result = BTreeMap::new();
    let mut current = None;
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(|e| e.to_string())?;
        if !is_word(&ns) {
            if matches!(event, Event::Eof) {
                break;
            }
            continue;
        }
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let attrs = attributes(&reader, e)?;
                match e.local_name().as_ref() {
                    "style" => {
                        current = attrs.get("styleId").cloned();
                        if let Some(id) = &current {
                            result.insert(id.clone(), style_id(id));
                        }
                    }
                    "name" => {
                        if let (Some(id), Some(name)) = (&current, attrs.get("val")) {
                            result.insert(id.clone(), style_id(name));
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) if e.local_name().as_ref() == "style" => current = None,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

struct DocumentFormatting {
    paragraphs: Vec<Paragraph>,
    tables: Vec<Table>,
    warnings: Vec<Warning>,
}

fn parse(
    xml: &[u8],
    names: &BTreeMap<String, StyleId>,
    limits: &Limits,
) -> Result<DocumentFormatting, String> {
    let mut reader = NsReader::from_reader(xml);
    let mut stack = Vec::<String>::new();
    let mut paragraphs = Vec::new();
    let mut tables = Vec::new();
    let mut active_tables = Vec::<usize>::new();
    let mut paragraph = None::<Paragraph>;
    let mut run = None::<Run>;
    let mut notices = Notices::default();
    let mut root_seen = false;
    let mut root_closed = false;
    let mut nodes = 0u64;
    let mut paragraph_count = 0u64;
    let mut text_box_depth = None;
    let mut text_boxes_seen = false;
    let node_limit = 8192u64.saturating_add(u64::from(limits.max_blocks).saturating_mul(64));
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(|e| e.to_string())?;
        let word = is_word(&ns);
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                nodes += 1;
                if nodes > node_limit {
                    return Err("DOCX document XML exceeds node limit".into());
                }
                if stack.len() >= usize::from(limits.max_nesting_depth) {
                    return Err("DOCX document XML exceeds depth limit".into());
                }
                let local = if word {
                    element.local_name().as_ref().to_owned()
                } else {
                    String::new()
                };
                if stack.is_empty() {
                    if root_seen || root_closed || local != "document" {
                        return Err("invalid DOCX document XML root".into());
                    }
                    root_seen = true;
                }
                let mut ancestry = stack.iter().rev().map(String::as_str);
                let ancestors = [
                    ancestry.next().unwrap_or(""),
                    ancestry.next().unwrap_or(""),
                    ancestry.next().unwrap_or(""),
                ];
                let paragraph_properties = ancestors.starts_with(&["pPr", "p"]);
                let run_properties = ancestors.starts_with(&["rPr", "r"]);
                let paragraph_run_properties = ancestors.starts_with(&["rPr", "pPr", "p"]);
                let attrs = attributes(&reader, element)?;
                if local == "p" {
                    paragraph_count += 1;
                    if paragraph_count > u64::from(limits.max_blocks) {
                        return Err("DOCX paragraph count exceeds block limit".into());
                    }
                }
                // Text boxes form separate Word stories inside an enclosing run.
                // Pandoc owns their semantic content and alternate-content choice;
                // do not mix their runs into the enclosing paragraph's formatting.
                if local == "txbxContent" && text_box_depth.is_none() {
                    text_box_depth = Some(stack.len());
                    text_boxes_seen = true;
                }
                match local.as_str() {
                    _ if text_box_depth.is_some() => {}
                    "tbl" => {
                        active_tables.push(tables.len());
                        tables.push(Table::default());
                    }
                    "p" => {
                        if paragraph.is_some() {
                            return Err("nested DOCX paragraphs".into());
                        }
                        paragraph = Some(Paragraph::default());
                    }
                    "r" if paragraph.is_some() => {
                        if run.is_some() {
                            return Err("nested DOCX runs".into());
                        }
                        run = Some(Run::default());
                    }
                    "pStyle" if paragraph_properties => {
                        if let Some(p) = &mut paragraph {
                            p.style = attrs.get("val").and_then(|id| names.get(id)).cloned();
                        }
                    }
                    "rStyle" if run_properties => {
                        if let Some(r) = &mut run {
                            r.style = attrs.get("val").and_then(|id| names.get(id)).cloned();
                        }
                    }
                    "tblStyle" if ancestors.starts_with(&["tblPr", "tbl"]) => {
                        if let Some(index) = active_tables.last() {
                            tables[*index].style =
                                attrs.get("val").and_then(|id| names.get(id)).cloned();
                        }
                    }
                    _ if paragraph_properties => {
                        if let Some(p) = &mut paragraph
                            && !matches!(local.as_str(), "rPr" | "numPr" | "sectPr")
                        {
                            paragraph_property(&local, &attrs, &mut p.direct, &mut notices)?;
                        }
                    }
                    _ if run_properties || paragraph_run_properties => {
                        if let Some(r) = &mut run {
                            character_property(&local, &attrs, &mut r.direct, &mut notices)?;
                        } else if let Some(p) = &mut paragraph {
                            character_property(&local, &attrs, &mut p.character, &mut notices)?;
                        }
                    }
                    "tab" | "br" | "cr" => {
                        if let Some(r) = &mut run {
                            r.text.push(if local == "tab" { '\t' } else { '\n' });
                        }
                    }
                    _ => {}
                }
                if matches!(event, Event::Start(_)) {
                    stack.push(local);
                } else {
                    match local.as_str() {
                        _ if text_box_depth.is_some() => {}
                        "p" => {
                            finish_paragraph(
                                &mut paragraph,
                                &mut paragraphs,
                                &mut tables,
                                &active_tables,
                                limits,
                            )?;
                        }
                        "r" => {
                            run = None;
                        }
                        "tbl" => {
                            active_tables.pop();
                        }
                        _ => {}
                    }
                    if text_box_depth == Some(stack.len()) {
                        text_box_depth = None;
                    }
                }
            }
            Event::Text(text) if stack.last().is_some_and(|name| name == "t") => {
                if text_box_depth.is_none()
                    && let Some(r) = &mut run
                {
                    r.text.push_str(&text.xml10_content());
                }
            }
            Event::GeneralRef(reference) if stack.last().is_some_and(|name| name == "t") => {
                let name = reference.xml10_content();
                let ch = match name.as_ref() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    _ => reference
                        .resolve_char_ref()
                        .map_err(|e| e.to_string())?
                        .ok_or("XML entities are forbidden in DOCX documents")?,
                };
                if text_box_depth.is_none()
                    && let Some(r) = &mut run
                {
                    r.text.push(ch);
                }
            }
            Event::End(_) => {
                let name = stack.pop().ok_or("unexpected DOCX XML end")?;
                let in_text_box = text_box_depth.is_some();
                if text_box_depth == Some(stack.len()) {
                    text_box_depth = None;
                }
                match name.as_str() {
                    _ if in_text_box => {}
                    "r" => finish_run(&mut paragraph, &mut run),
                    "p" => finish_paragraph(
                        &mut paragraph,
                        &mut paragraphs,
                        &mut tables,
                        &active_tables,
                        limits,
                    )?,
                    "tbl" => {
                        active_tables.pop();
                    }
                    _ => {}
                }
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            Event::DocType(_) | Event::CData(_) | Event::GeneralRef(_) => {
                return Err("XML entities, DTDs and CDATA are forbidden in DOCX documents".into());
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || !root_closed || !stack.is_empty() {
        return Err("incomplete DOCX document XML".into());
    }
    let mut warnings = notices.warnings();
    if text_boxes_seen {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "DOCX text box content and formatting may be omitted by the semantic reader.",
        ));
    }
    Ok(DocumentFormatting {
        paragraphs,
        tables,
        warnings,
    })
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text: value } | Inline::Code { text: value } => text.push_str(value),
            Inline::Styled { content, .. }
            | Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => text.push_str(&inline_text(content)),
            Inline::SoftBreak {} => text.push(' '),
            Inline::LineBreak {} => text.push('\n'),
            _ => {}
        }
    }
    text
}
fn key(text: &str) -> String {
    text.nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph { content, .. } | Block::Heading { content, .. } => inline_text(content),
        Block::Table { head, body, .. } => head
            .iter()
            .chain(body)
            .flatten()
            .flat_map(|cell| &cell.blocks)
            .map(block_text)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

struct RunDecorator<'a> {
    runs: &'a [Run],
    run_index: usize,
    run_offset: usize,
}

impl<'a> RunDecorator<'a> {
    fn new(runs: &'a [Run]) -> Self {
        Self {
            runs,
            run_index: 0,
            run_offset: 0,
        }
    }

    fn current(&mut self) -> Option<(&'a Run, usize)> {
        while let Some(run) = self.runs.get(self.run_index) {
            let remaining = run.text.len().saturating_sub(self.run_offset);
            if remaining > 0 {
                return Some((run, remaining));
            }
            self.run_index += 1;
            self.run_offset = 0;
        }
        None
    }

    fn advance(&mut self, mut count: usize) {
        while count > 0 {
            let Some((_, remaining)) = self.current() else {
                return;
            };
            let consumed = count.min(remaining);
            self.run_offset += consumed;
            count -= consumed;
        }
    }

    fn decorate_text(&mut self, text: &str, result: &mut Vec<Inline>) {
        let mut remaining = text;
        while !remaining.is_empty() {
            let Some((run, run_remaining)) = self.current() else {
                result.push(Inline::Text {
                    text: remaining.to_owned(),
                });
                return;
            };
            let count = run_remaining.min(remaining.len());
            if !remaining.is_char_boundary(count) {
                result.push(Inline::Text {
                    text: remaining.to_owned(),
                });
                return;
            }

            let content = vec![Inline::Text {
                text: remaining[..count].to_owned(),
            }];
            if run.style.is_some() || !run.direct.is_empty() {
                result.push(Inline::Styled {
                    style: run.style.clone(),
                    direct: run.direct.clone(),
                    content,
                });
            } else {
                result.extend(content);
            }
            self.advance(count);
            remaining = &remaining[count..];
        }
    }

    fn decorate(&mut self, inlines: &mut Vec<Inline>) {
        let mut result = Vec::new();
        for mut inline in std::mem::take(inlines) {
            match &mut inline {
                Inline::Text { text } => {
                    self.decorate_text(text, &mut result);
                    continue;
                }
                Inline::Styled { content, .. }
                | Inline::Emph { content }
                | Inline::Strong { content }
                | Inline::Strikeout { content }
                | Inline::Superscript { content }
                | Inline::Subscript { content }
                | Inline::Link { content, .. } => self.decorate(content),
                _ => self.advance(inline_text(std::slice::from_ref(&inline)).len()),
            }
            result.push(inline);
        }
        *inlines = result;
    }
}

/// Applies formatting only to matching semantic text; opaque/unmatched content stays intact.
pub(super) fn apply(
    document: &mut Document,
    bytes: &[u8],
    limits: &Limits,
) -> Result<Vec<Warning>, String> {
    let names = names(&member(bytes, "word/styles.xml", limits)?)?;
    let xml = member(bytes, "word/document.xml", limits)?;
    if xml.is_empty() {
        return Ok(Vec::new());
    }
    let DocumentFormatting {
        paragraphs,
        tables,
        mut warnings,
    } = parse(&xml, &names, limits)?;
    let mut paragraphs: BTreeMap<String, VecDeque<_>> =
        paragraphs.into_iter().fold(BTreeMap::new(), |mut map, p| {
            map.entry(key(&p.text)).or_default().push_back(p);
            map
        });
    let mut tables: BTreeMap<String, VecDeque<_>> =
        tables.into_iter().fold(BTreeMap::new(), |mut map, t| {
            map.entry(key(&t.text)).or_default().push_back(t);
            map
        });
    let mut stack: Vec<_> = document.body.iter_mut().rev().collect();
    while let Some(block) = stack.pop() {
        let text = block_text(block);
        match block {
            Block::Paragraph {
                content,
                style,
                direct,
                ..
            }
            | Block::Heading {
                content,
                style,
                direct,
                ..
            } => {
                if let Some(p) = paragraphs
                    .get_mut(&key(&text))
                    .and_then(VecDeque::pop_front)
                {
                    // Pandoc can emit a semantic Header without its Word style.
                    // Replace the mapper's paragraph-run companion after recovering
                    // the actual pStyle, while preserving nested explicit run overrides.
                    let old_run = style.as_ref().map(paragraph_run_id);
                    if let [
                        Inline::Styled {
                            style: Some(run),
                            direct,
                            ..
                        },
                    ] = content.as_slice()
                        && old_run.as_ref() == Some(run)
                        && direct.is_empty()
                        && let Inline::Styled { content: inner, .. } = content.remove(0)
                    {
                        *content = inner;
                    }
                    if let Some(named) = p
                        .style
                        .filter(|id| document.styles.paragraph.contains_key(id))
                    {
                        *style = Some(named);
                    }
                    direct.overlay(&p.direct);
                    if text == p.text {
                        RunDecorator::new(&p.runs).decorate(content);
                    } else if p
                        .runs
                        .iter()
                        .any(|r| r.style.is_some() || !r.direct.is_empty())
                    {
                        warnings.push(Warning::new(WarningCode::UnsupportedNode,
                            "DOCX run formatting was not applied where semantic whitespace changed."));
                    }
                    if !p.character.is_empty() {
                        *content = vec![Inline::Styled {
                            style: None,
                            direct: p.character,
                            content: std::mem::take(content),
                        }];
                    }
                    if let Some(run) = style
                        .as_ref()
                        .map(paragraph_run_id)
                        .filter(|run| document.styles.character.contains_key(run))
                    {
                        *content = vec![Inline::Styled {
                            style: Some(run),
                            direct: Default::default(),
                            content: std::mem::take(content),
                        }];
                    }
                }
            }
            Block::Table {
                head, body, style, ..
            } => {
                if let Some(t) = tables.get_mut(&key(&text)).and_then(VecDeque::pop_front)
                    && let Some(named) = t.style.filter(|id| document.styles.table.contains_key(id))
                {
                    *style = Some(named);
                }
                for cell in head.iter_mut().chain(body).flatten().rev() {
                    stack.extend(cell.blocks.iter_mut().rev());
                }
            }
            Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
                stack.extend(blocks.iter_mut().rev())
            }
            Block::List { items, .. } => {
                for item in items.iter_mut().rev() {
                    stack.extend(item.blocks.iter_mut().rev());
                }
            }
            _ => {}
        }
    }
    if paragraphs.values().flatten().any(|p| {
        p.style.is_some()
            || !p.direct.is_empty()
            || !p.character.is_empty()
            || p.runs
                .iter()
                .any(|r| r.style.is_some() || !r.direct.is_empty())
    }) || tables.values().flatten().any(|t| t.style.is_some())
    {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "Some DOCX formatting could not be matched to semantic paragraphs.",
        ));
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT_BOX: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml"><w:body>
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>before </w:t><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:rPr><w:i/></w:rPr><w:t>box &amp; story</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict><w:t>after</w:t></w:r></w:p>
<w:p><w:r><w:t>tail</w:t></w:r></w:p></w:body></w:document>"#;

    #[test]
    fn text_box_stories_do_not_contaminate_enclosing_run_formatting() {
        let empty = TEXT_BOX.replace(
            "<w:txbxContent><w:p><w:r><w:rPr><w:i/></w:rPr><w:t>box &amp; story</w:t></w:r></w:p></w:txbxContent>",
            "<w:txbxContent/>",
        );
        for xml in [TEXT_BOX, empty.as_str()] {
            let formatting = parse(xml.as_bytes(), &BTreeMap::new(), &Limits::local()).unwrap();
            assert_eq!(formatting.paragraphs.len(), 2);
            assert_eq!(formatting.paragraphs[0].text, "before after");
            assert_eq!(formatting.paragraphs[0].runs.len(), 1);
            assert_eq!(formatting.paragraphs[0].runs[0].direct.weight, Some(700));
            assert_eq!(formatting.paragraphs[0].runs[0].direct.italic, None);
            assert_eq!(formatting.paragraphs[1].text, "tail");
            assert_eq!(formatting.warnings.len(), 1);
            assert_eq!(formatting.warnings[0].code, WarningCode::UnsupportedNode);
        }
    }

    #[test]
    fn text_box_xml_retains_paragraph_depth_and_entity_limits() {
        let names = BTreeMap::new();
        let mut limits = Limits::local();
        limits.max_blocks = 2;
        assert!(
            parse(TEXT_BOX.as_bytes(), &names, &limits)
                .err()
                .unwrap()
                .contains("paragraph count")
        );
        limits = Limits::local();
        limits.max_nesting_depth = 8;
        assert!(
            parse(TEXT_BOX.as_bytes(), &names, &limits)
                .err()
                .unwrap()
                .contains("depth limit")
        );
        assert!(
            parse(
                TEXT_BOX.replace("&amp;", "&unknown;").as_bytes(),
                &names,
                &Limits::local()
            )
            .is_err()
        );
    }

    #[test]
    fn nested_paragraphs_outside_text_box_stories_remain_invalid() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:p/></w:r></w:p></w:body></w:document>"#;
        assert!(
            parse(xml, &BTreeMap::new(), &Limits::local())
                .err()
                .unwrap()
                .contains("nested DOCX paragraphs")
        );
    }
}
