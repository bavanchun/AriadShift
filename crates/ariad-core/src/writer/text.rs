//! Plain text document writer producing human-readable text.

use crate::{
    ir::{Block, Document, Inline, TableCell},
    limits::Limits,
    warning::{Warning, WarningCode},
    writer::{WriteOutput, normalize_document},
};

const MAX_TABLE_COLUMNS: usize = 128;
const MAX_WRITER_NESTING_DEPTH: usize = 100;

pub(super) fn output_ceiling_exceeded() -> WriteOutput {
    WriteOutput {
        content: String::new(),
        warnings: vec![Warning::new(
            WarningCode::UnsupportedNode,
            "document exceeds the text writer output ceiling (512 MiB) or nesting limit",
        )],
    }
}

/// Serializes `document` into human-readable plain text.
#[must_use]
pub fn write(document: &Document) -> WriteOutput {
    let doc = normalize_document(document);
    let mut writer = TextWriter::new();

    if !writer.write_blocks(&doc.body, 0) {
        return output_ceiling_exceeded();
    }

    if !doc.furniture.is_empty() {
        if !writer.ensure_blank_line() {
            return output_ceiling_exceeded();
        }
        if !writer.write_blocks(&doc.furniture, 0) {
            return output_ceiling_exceeded();
        }
    }

    let mut content = writer.out;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }

    WriteOutput {
        content,
        warnings: writer.warnings,
    }
}

struct TextWriter {
    out: String,
    warnings: Vec<Warning>,
    remaining_bytes: usize,
}

impl TextWriter {
    fn new() -> Self {
        Self {
            out: String::new(),
            warnings: Vec::new(),
            remaining_bytes: Limits::MAX_TEXT_OUTPUT_BYTES,
        }
    }

    fn push_str(&mut self, s: &str) -> bool {
        if s.len() > self.remaining_bytes {
            return false;
        }
        self.remaining_bytes -= s.len();
        self.out.push_str(s);
        true
    }

    fn push_char(&mut self, c: char) -> bool {
        let len = c.len_utf8();
        if len > self.remaining_bytes {
            return false;
        }
        self.remaining_bytes -= len;
        self.out.push(c);
        true
    }

    fn warn(&mut self, code: WarningCode, message: impl Into<String>) {
        self.warnings.push(Warning::new(code, message));
    }

    fn ensure_blank_line(&mut self) -> bool {
        if self.out.is_empty() || self.out.ends_with("\n\n") {
            true
        } else if self.out.ends_with('\n') {
            self.push_char('\n')
        } else {
            self.push_str("\n\n")
        }
    }

    fn write_blocks(&mut self, blocks: &[Block], depth: usize) -> bool {
        if depth > MAX_WRITER_NESTING_DEPTH {
            return false;
        }
        for (i, block) in blocks.iter().enumerate() {
            if i > 0 && !self.ensure_blank_line() {
                return false;
            }
            if !self.write_block(block, depth) {
                return false;
            }
        }
        true
    }

    fn write_block(&mut self, block: &Block, depth: usize) -> bool {
        if depth > MAX_WRITER_NESTING_DEPTH {
            return false;
        }
        match block {
            Block::Heading { level, content } => {
                let mut temp = Vec::new();
                let heading_text = inlines_to_string(content, &mut temp);
                self.warnings.extend(temp);
                let hashes = "#".repeat((*level as usize).clamp(1, 6));
                self.push_str(&format!("{hashes} {heading_text}\n\n"))
            }
            Block::Paragraph { content } => {
                if !self.write_inlines(content) {
                    return false;
                }
                self.push_str("\n\n")
            }
            Block::Code { lang: _, text } => {
                if !self.push_str(text) {
                    return false;
                }
                self.push_str("\n\n")
            }
            Block::Math { tex, display: _ } => {
                self.warn(
                    WarningCode::MathNotConverted,
                    "TeX math could not be converted for the target and was kept as text",
                );
                if !self.push_str(tex) {
                    return false;
                }
                self.push_str("\n\n")
            }
            Block::Quote { blocks } => {
                let mut inner = TextWriter::new();
                inner.remaining_bytes = self.remaining_bytes;
                if !inner.write_blocks(blocks, depth + 1) {
                    return false;
                }
                self.warnings.extend(inner.warnings);
                for line in inner.out.lines() {
                    if line.is_empty() {
                        if !self.push_str(">\n") {
                            return false;
                        }
                    } else if !self.push_str(&format!("> {line}\n")) {
                        return false;
                    }
                }
                self.push_char('\n')
            }
            Block::PageBreak {} => self.push_str("\n---\n\n"),
            Block::List {
                ordered,
                start,
                tight: _,
                items,
            } => {
                let mut num = start.unwrap_or(1);
                for item in items {
                    let marker = if *ordered {
                        let m = format!("{num}. ");
                        num += 1;
                        m
                    } else {
                        "- ".to_string()
                    };
                    let check = match item.checked {
                        Some(true) => "[x] ",
                        Some(false) => "[ ] ",
                        None => "",
                    };
                    let prefix = format!("{marker}{check}");
                    let indent = " ".repeat(prefix.len());
                    let mut inner = TextWriter::new();
                    inner.remaining_bytes = self.remaining_bytes;
                    if !inner.write_blocks(&item.blocks, depth + 1) {
                        return false;
                    }
                    self.warnings.extend(inner.warnings);
                    for (li, line) in inner.out.lines().enumerate() {
                        if li == 0 {
                            if !self.push_str(&format!("{prefix}{line}\n")) {
                                return false;
                            }
                        } else if line.is_empty() {
                            if !self.push_char('\n') {
                                return false;
                            }
                        } else if !self.push_str(&format!("{indent}{line}\n")) {
                            return false;
                        }
                    }
                }
                self.push_char('\n')
            }
            Block::Table {
                caption,
                columns: _,
                head,
                body,
                footnotes,
            } => {
                if let Some(caption_inlines) = caption {
                    let mut temp = Vec::new();
                    let cap_str = inlines_to_string(caption_inlines, &mut temp);
                    self.warnings.extend(temp);
                    if !self.push_str(&format!("Table: {cap_str}\n")) {
                        return false;
                    }
                }

                // Render cell texts
                let mut head_rows = Vec::new();
                for row in head {
                    let mut rendered_row = Vec::new();
                    for cell in row {
                        rendered_row.push(cell_to_string(cell, &mut self.warnings));
                    }
                    head_rows.push(rendered_row);
                }

                let mut body_rows = Vec::new();
                for row in body {
                    let mut rendered_row = Vec::new();
                    for cell in row {
                        rendered_row.push(cell_to_string(cell, &mut self.warnings));
                    }
                    body_rows.push(rendered_row);
                }

                let col_count = head_rows
                    .iter()
                    .chain(body_rows.iter())
                    .map(|r| r.len())
                    .max()
                    .unwrap_or(0)
                    .min(MAX_TABLE_COLUMNS);

                if col_count > 0 {
                    let mut col_widths = vec![3usize; col_count];
                    for row in head_rows.iter().chain(body_rows.iter()) {
                        for (c, cell_str) in row.iter().enumerate().take(col_count) {
                            col_widths[c] = col_widths[c].max(cell_str.chars().count());
                        }
                    }

                    // Write head rows
                    for row in &head_rows {
                        if !self.write_table_row(row, &col_widths, col_count) {
                            return false;
                        }
                    }

                    // Write separator
                    if !head_rows.is_empty() {
                        let sep_parts: Vec<String> =
                            col_widths.iter().map(|&w| "-".repeat(w.max(3))).collect();
                        if !self.push_str(&format!("| {} |\n", sep_parts.join(" | "))) {
                            return false;
                        }
                    }

                    // Write body rows
                    for row in &body_rows {
                        if !self.write_table_row(row, &col_widths, col_count) {
                            return false;
                        }
                    }
                    self.push_char('\n');
                }

                if !footnotes.is_empty() {
                    let mut temp = Vec::new();
                    let fn_str = inlines_to_string(footnotes, &mut temp);
                    self.warnings.extend(temp);
                    if !self.push_str(&format!("Footnotes: {fn_str}\n\n")) {
                        return false;
                    }
                }

                true
            }
            Block::Figure { asset: _, caption } => {
                self.warn(
                    WarningCode::ImageNotEmbedded,
                    "figure cannot be rendered in plain text",
                );
                if !caption.is_empty() {
                    let mut temp = Vec::new();
                    let cap_text = inlines_to_string(caption, &mut temp);
                    self.warnings.extend(temp);
                    self.push_str(&format!("[Figure: {cap_text}]\n\n"))
                } else {
                    true
                }
            }
            Block::Footnote { id, blocks } => {
                let mut inner = TextWriter::new();
                inner.remaining_bytes = self.remaining_bytes;
                if !inner.write_blocks(blocks, depth + 1) {
                    return false;
                }
                self.warnings.extend(inner.warnings);
                let trimmed = inner.out.trim();
                self.push_str(&format!("[^{id}]: {trimmed}\n\n"))
            }
            Block::Raw { format, text: _ } => {
                self.warn(
                    WarningCode::RawDropped,
                    format!("raw {format:?} block dropped"),
                );
                true
            }
        }
    }

    fn write_table_row(&mut self, row: &[String], col_widths: &[usize], col_count: usize) -> bool {
        let mut padded = Vec::with_capacity(col_count);
        for (c, &width) in col_widths.iter().enumerate().take(col_count) {
            let cell = row.get(c).map(String::as_str).unwrap_or("");
            let char_count = cell.chars().count();
            let pad = if width > char_count {
                " ".repeat(width - char_count)
            } else {
                String::new()
            };
            padded.push(format!("{cell}{pad}"));
        }
        self.push_str(&format!("| {} |\n", padded.join(" | ")))
    }

    fn write_inlines(&mut self, inlines: &[Inline]) -> bool {
        for inline in inlines {
            if !self.write_inline(inline) {
                return false;
            }
        }
        true
    }

    fn write_inline(&mut self, inline: &Inline) -> bool {
        match inline {
            Inline::Text { text } => self.push_str(text),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Subscript { content } => self.write_inlines(content),
            Inline::Superscript { content } => {
                if !self.push_char('^') {
                    return false;
                }
                self.write_inlines(content)
            }
            Inline::Code { text } => self.push_str(text),
            Inline::Link {
                url,
                title: _,
                content,
            } => {
                self.warn(
                    WarningCode::LinkDropped,
                    format!("link target `{url}` was reduced to its text"),
                );
                self.write_inlines(content)
            }
            Inline::Image {
                target: _,
                alt,
                title: _,
            } => {
                self.warn(
                    WarningCode::ImageNotEmbedded,
                    "image cannot be rendered in plain text",
                );
                if !alt.is_empty() {
                    self.push_str(alt)
                } else {
                    self.push_str("[Image]")
                }
            }
            Inline::SoftBreak {} => self.push_char(' '),
            Inline::LineBreak {} => self.push_char('\n'),
            Inline::Math { tex, display: _ } => {
                self.warn(
                    WarningCode::MathNotConverted,
                    "TeX math could not be converted for the target and was kept as text",
                );
                self.push_str(tex)
            }
            Inline::FootnoteRef { id } => {
                let ref_str = format!("[^{id}]");
                self.push_str(&ref_str)
            }
            Inline::Raw { format, text: _ } => {
                self.warn(
                    WarningCode::RawDropped,
                    format!("raw {format:?} content dropped"),
                );
                true
            }
        }
    }
}

fn inlines_to_string(inlines: &[Inline], warnings: &mut Vec<Warning>) -> String {
    let mut writer = TextWriter::new();
    let _ = writer.write_inlines(inlines);
    warnings.extend(writer.warnings);
    writer.out
}

fn cell_to_string(cell: &TableCell, warnings: &mut Vec<Warning>) -> String {
    let mut writer = TextWriter::new();
    let _ = writer.write_blocks(&cell.blocks, 0);
    warnings.extend(writer.warnings);
    writer.out.trim().replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Alignment, ColumnSpec, ListItem, Metadata, RawFormat, TableCell};

    #[test]
    fn roundtrips_headings_lists_and_tables_to_readable_text() {
        let doc = Document {
            version: crate::ir::IR_VERSION.to_owned(),
            meta: Metadata::default(),
            body: vec![
                Block::Heading {
                    level: 1,
                    content: vec![Inline::Text {
                        text: "Introduction".into(),
                    }],
                },
                Block::Paragraph {
                    content: vec![
                        Inline::Text {
                            text: "Here is a ".into(),
                        },
                        Inline::Strong {
                            content: vec![Inline::Text {
                                text: "bold".into(),
                            }],
                        },
                        Inline::Text {
                            text: " statement.".into(),
                        },
                    ],
                },
                Block::List {
                    ordered: false,
                    start: None,
                    tight: true,
                    items: vec![
                        ListItem {
                            checked: None,
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "First item".into(),
                                }],
                            }],
                        },
                        ListItem {
                            checked: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Checked task".into(),
                                }],
                            }],
                        },
                    ],
                },
                Block::Table {
                    caption: Some(vec![Inline::Text {
                        text: "Sample Data".into(),
                    }]),
                    columns: vec![
                        ColumnSpec {
                            align: Alignment::Default,
                        },
                        ColumnSpec {
                            align: Alignment::Default,
                        },
                    ],
                    head: vec![vec![
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text { text: "Key".into() }],
                            }],
                        },
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Value".into(),
                                }],
                            }],
                        },
                    ]],
                    body: vec![vec![
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(false),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Alpha".into(),
                                }],
                            }],
                        },
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(false),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text { text: "100".into() }],
                            }],
                        },
                    ]],
                    footnotes: vec![],
                },
            ],
            assets: Default::default(),
            layout: None,
            provenance: None,
            furniture: vec![],
        };

        let output = write(&doc);
        assert!(output.content.contains("# Introduction\n\n"));
        assert!(output.content.contains("Here is a bold statement.\n\n"));
        assert!(output.content.contains("- First item\n"));
        assert!(output.content.contains("- [x] Checked task\n"));
        assert!(output.content.contains("Table: Sample Data\n"));
        assert!(output.content.contains("| Key   | Value |\n"));
        assert!(output.content.contains("| ----- | ----- |\n"));
        assert!(output.content.contains("| Alpha | 100   |\n"));
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn emits_warnings_for_dropped_content() {
        let doc = Document {
            version: crate::ir::IR_VERSION.to_owned(),
            meta: Metadata::default(),
            body: vec![
                Block::Paragraph {
                    content: vec![
                        Inline::Link {
                            url: "https://example.com".into(),
                            title: None,
                            content: vec![Inline::Text {
                                text: "Example Link".into(),
                            }],
                        },
                        Inline::Image {
                            target: crate::ir::AssetRef::Url {
                                href: "test.png".into(),
                            },
                            alt: "Diagram".into(),
                            title: None,
                        },
                        Inline::Math {
                            tex: "x^2 + y^2 = z^2".into(),
                            display: false,
                        },
                        Inline::Raw {
                            format: RawFormat::Html,
                            text: "<span>dropped</span>".into(),
                        },
                    ],
                },
                Block::Raw {
                    format: RawFormat::Html,
                    text: "<div>dropped block</div>".into(),
                },
                Block::Figure {
                    asset: crate::ir::AssetRef::Url {
                        href: "fig.png".into(),
                    },
                    caption: vec![Inline::Text {
                        text: "A figure".into(),
                    }],
                },
            ],
            assets: Default::default(),
            layout: None,
            provenance: None,
            furniture: vec![],
        };

        let output = write(&doc);
        assert!(output.content.contains("Example Link"));
        assert!(output.content.contains("Diagram"));
        assert!(output.content.contains("x^2 + y^2 = z^2"));
        assert!(output.content.contains("[Figure: A figure]"));

        let warning_codes: Vec<WarningCode> = output.warnings.iter().map(|w| w.code).collect();
        assert!(warning_codes.contains(&WarningCode::LinkDropped));
        assert!(warning_codes.contains(&WarningCode::ImageNotEmbedded));
        assert!(warning_codes.contains(&WarningCode::MathNotConverted));
        assert!(warning_codes.contains(&WarningCode::RawDropped));
    }

    #[test]
    fn formats_headings_lists_tables_and_footnotes() {
        let doc = Document {
            version: crate::ir::IR_VERSION.to_owned(),
            meta: Metadata::default(),
            body: vec![
                Block::Heading {
                    level: 2,
                    content: vec![Inline::Text {
                        text: "Detailed Section".into(),
                    }],
                },
                Block::Paragraph {
                    content: vec![
                        Inline::Text {
                            text: "Referencing a footnote".into(),
                        },
                        Inline::FootnoteRef { id: "fn1".into() },
                        Inline::Text {
                            text: " in text.".into(),
                        },
                    ],
                },
                Block::List {
                    ordered: true,
                    start: Some(10),
                    tight: true,
                    items: vec![
                        ListItem {
                            checked: Some(false),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Pending item".into(),
                                }],
                            }],
                        },
                        ListItem {
                            checked: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Finished item".into(),
                                }],
                            }],
                        },
                    ],
                },
                Block::Table {
                    caption: Some(vec![Inline::Text {
                        text: "Metrics Table".into(),
                    }]),
                    columns: vec![
                        ColumnSpec {
                            align: Alignment::Left,
                        },
                        ColumnSpec {
                            align: Alignment::Right,
                        },
                    ],
                    head: vec![vec![
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Metric".into(),
                                }],
                            }],
                        },
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Score".into(),
                                }],
                            }],
                        },
                    ]],
                    body: vec![vec![
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(false),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "Fidelity".into(),
                                }],
                            }],
                        },
                        TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: Some(false),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "99.5%".into(),
                                }],
                            }],
                        },
                    ]],
                    footnotes: vec![Inline::Text {
                        text: "Measured against golden standards.".into(),
                    }],
                },
                Block::Footnote {
                    id: "fn1".into(),
                    blocks: vec![Block::Paragraph {
                        content: vec![Inline::Text {
                            text: "Footnote explanation text.".into(),
                        }],
                    }],
                },
            ],
            assets: Default::default(),
            layout: None,
            provenance: None,
            furniture: vec![],
        };

        let output = write(&doc);
        assert!(output.content.contains("## Detailed Section\n\n"));
        assert!(
            output
                .content
                .contains("Referencing a footnote[^fn1] in text.\n\n")
        );
        assert!(output.content.contains("10. [ ] Pending item\n"));
        assert!(output.content.contains("11. [x] Finished item\n"));
        assert!(output.content.contains("Table: Metrics Table\n"));
        assert!(output.content.contains("| Metric   | Score |\n"));
        assert!(output.content.contains("| -------- | ----- |\n"));
        assert!(output.content.contains("| Fidelity | 99.5% |\n"));
        assert!(
            output
                .content
                .contains("Footnotes: Measured against golden standards.\n\n")
        );
        assert!(
            output
                .content
                .contains("[^fn1]: Footnote explanation text.\n\n")
        );
        assert!(output.warnings.is_empty());
    }
}
