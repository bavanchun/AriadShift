//! Explicit wire-version boundaries. Legacy bytes never depend on current derives.
use super::{Block, Document, legacy};
use serde::{
    Serialize,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::Value;

impl Document {
    /// Serializes through the frozen v1 representation, dropping v2-only data.
    pub fn to_v1(&self) -> Result<legacy::Document, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        let paths: std::collections::BTreeMap<_, _> = super::identity::blocks_with_paths(self)
            .into_iter()
            .map(|(path, block)| (block.id().to_string(), path))
            .collect();
        if let Some(blocks) = value
            .get_mut("layout")
            .and_then(|v| v.get_mut("blocks"))
            .and_then(Value::as_object_mut)
        {
            *blocks = std::mem::take(blocks)
                .into_iter()
                .map(|(id, pos)| (paths.get(&id).cloned().unwrap_or(id), pos))
                .collect();
        }
        value["version"] = Value::String(legacy::IR_VERSION.to_owned());
        if let Some(object) = value.as_object_mut() {
            for key in ["styles", "sections", "source_map"] {
                object.remove(key);
            }
        }
        visit_document_blocks(&mut value, &mut downgrade_block_value);
        if let Some(assets) = value.get_mut("assets").and_then(Value::as_object_mut) {
            assets.retain(|id, _| {
                self.assets
                    .get(id)
                    .is_some_and(super::Asset::can_export_bytes)
            });
            for asset in assets.values_mut() {
                if let Some(asset) = asset.as_object_mut() {
                    asset.remove("font");
                }
            }
        }
        serde_json::from_value(value)
    }
    /// Removes prohibited embedded font bytes at the export boundary.
    pub fn without_prohibited_fonts(&self) -> (Self, bool) {
        let mut document = self.clone();
        let before = document.assets.len();
        document.assets.retain(|_, asset| asset.can_export_bytes());
        let dropped = before != document.assets.len();
        (document, dropped)
    }
}

pub(super) fn semantic_payload(block: &Block) -> Vec<u8> {
    serde_json::to_vec(&SemanticBlock(block)).expect("semantic payload serializes")
}

struct SemanticBlock<'a>(&'a Block);
impl Serialize for SemanticBlock<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let block = self.0;
        if let Block::Table {
            caption,
            columns,
            head,
            body,
            footnotes,
            ..
        } = block
        {
            let mut map = serializer.serialize_map(None)?;
            map.serialize_entry("body", &SemanticRows(body))?;
            map.serialize_entry("caption", &caption.as_deref().map(SemanticInlines))?;
            map.serialize_entry("columns", columns)?;
            if !footnotes.is_empty() {
                map.serialize_entry("footnotes", &SemanticInlines(footnotes))?;
            }
            map.serialize_entry("head", &SemanticRows(head))?;
            map.serialize_entry("type", "table")?;
            return map.end();
        }
        // Table-heavy imports contain many plain paragraphs. Serialize this common
        // shape directly in canonical key order instead of allocating a JSON tree.
        if let Block::Paragraph { content, .. } = block
            && let [super::Inline::Text { text }] = content.as_slice()
        {
            #[derive(serde::Serialize)]
            struct Text<'a> {
                text: &'a str,
                #[serde(rename = "type")]
                kind: &'static str,
            }
            #[derive(serde::Serialize)]
            struct Paragraph<'a> {
                content: [Text<'a>; 1],
                #[serde(rename = "type")]
                kind: &'static str,
            }
            return Paragraph {
                content: [Text { text, kind: "text" }],
                kind: "paragraph",
            }
            .serialize(serializer);
        }
        let mut value = serde_json::to_value(block).map_err(serde::ser::Error::custom)?;
        downgrade_block(&mut value);
        value.serialize(serializer)
    }
}

struct SemanticBlocks<'a>(&'a [Block]);
impl Serialize for SemanticBlocks<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for block in self.0 {
            seq.serialize_element(&SemanticBlock(block))?;
        }
        seq.end()
    }
}
struct SemanticRows<'a>(&'a [Vec<super::TableCell>]);
impl Serialize for SemanticRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            seq.serialize_element(&SemanticRow(row))?;
        }
        seq.end()
    }
}
struct SemanticRow<'a>(&'a [super::TableCell]);
impl Serialize for SemanticRow<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for cell in self.0 {
            seq.serialize_element(&SemanticCell(cell))?;
        }
        seq.end()
    }
}
struct SemanticCell<'a>(&'a super::TableCell);
impl Serialize for SemanticCell<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let cell = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("blocks", &SemanticBlocks(&cell.blocks))?;
        map.serialize_entry("colspan", &cell.colspan)?;
        if let Some(header) = cell.header {
            map.serialize_entry("header", &header)?;
        }
        map.serialize_entry("rowspan", &cell.rowspan)?;
        map.end()
    }
}
struct SemanticInlines<'a>(&'a [super::Inline]);
impl Serialize for SemanticInlines<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(self.0).map_err(serde::ser::Error::custom)?;
        downgrade_inlines(value.as_array_mut().expect("inlines serialize as an array"));
        value.serialize(serializer)
    }
}

fn downgrade_block(block: &mut Value) {
    visit_blocks(std::slice::from_mut(block), &mut downgrade_block_value);
}

fn downgrade_block_value(block: &mut Value) {
    if let Some(object) = block.as_object_mut() {
        for key in ["id", "style", "direct"] {
            object.remove(key);
        }
        if object.get("type").and_then(Value::as_str) == Some("footnote")
            && let Some(note_id) = object.remove("note_id")
        {
            object.insert("id".into(), note_id);
        }
    }
    for field in ["content", "caption", "footnotes"] {
        if let Some(inlines) = block.get_mut(field).and_then(Value::as_array_mut) {
            downgrade_inlines(inlines);
        }
    }
}

fn downgrade_inlines(inlines: &mut Vec<Value>) {
    let mut result = Vec::new();
    for mut inline in std::mem::take(inlines) {
        if let Some(content) = inline.get_mut("content").and_then(Value::as_array_mut) {
            downgrade_inlines(content);
        }
        if inline.get("type").and_then(Value::as_str) == Some("styled") {
            if let Some(content) = inline.get_mut("content").and_then(Value::as_array_mut) {
                result.append(content);
            }
        } else {
            result.push(inline);
        }
    }
    *inlines = result;
}

/// Converts a probed, bounded legacy document to the current in-memory representation.
pub(crate) fn upgrade_legacy(document: legacy::Document) -> Result<Document, serde_json::Error> {
    let mut value = serde_json::to_value(document)?;
    value["version"] = Value::String(super::IR_VERSION.to_owned());
    visit_document_blocks(&mut value, &mut upgrade_block);
    // Legacy layout paths are intentionally not deserialized as validated v2 IDs.
    let layout = value.as_object_mut().and_then(|v| v.remove("layout"));
    let mut document: Document = serde_json::from_value(value)?;
    if let Some(Value::Object(mut layout)) = layout {
        let blocks = layout
            .remove("blocks")
            .unwrap_or_else(|| Value::Object(Default::default()));
        let mut index: super::LayoutIndex = serde_json::from_value(Value::Object(layout))?;
        if let Value::Object(blocks) = blocks {
            for (path, pos) in blocks {
                index.blocks.insert(
                    super::BlockId::import_path(path),
                    serde_json::from_value(pos)?,
                );
            }
        }
        document.layout = Some(index);
    }
    let mut stack: Vec<_> = document
        .body
        .iter_mut()
        .chain(&mut document.furniture)
        .collect();
    while let Some(block) = stack.pop() {
        *block.id_mut() = super::BlockId::default();
        match block {
            Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => stack.extend(blocks),
            Block::List { items, .. } => {
                for item in items {
                    stack.extend(&mut item.blocks);
                }
            }
            Block::Table { head, body, .. } => {
                for row in head.iter_mut().chain(body) {
                    for cell in row {
                        stack.extend(&mut cell.blocks);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(document)
}
fn upgrade_block(block: &mut Value) {
    if block.get("type").and_then(Value::as_str) == Some("footnote")
        && let Some(id) = block.as_object_mut().and_then(|b| b.remove("id"))
    {
        block["note_id"] = id;
    }
    // A valid temporary ID permits the current derive; import finalization assigns real IDs below.
    block["id"] = Value::String("unassigned".into());
}

fn visit_document_blocks(value: &mut Value, visitor: &mut impl FnMut(&mut Value)) {
    for field in ["body", "furniture"] {
        if let Some(blocks) = value.get_mut(field).and_then(Value::as_array_mut) {
            visit_blocks(blocks, visitor);
        }
    }
}

fn visit_blocks(blocks: &mut [Value], visitor: &mut impl FnMut(&mut Value)) {
    for block in blocks {
        visitor(block);

        if let Some(blocks) = block.get_mut("blocks").and_then(Value::as_array_mut) {
            visit_blocks(blocks, visitor);
        }
        if let Some(items) = block.get_mut("items").and_then(Value::as_array_mut) {
            for item in items {
                if let Some(blocks) = item.get_mut("blocks").and_then(Value::as_array_mut) {
                    visit_blocks(blocks, visitor);
                }
            }
        }
        for field in ["head", "body"] {
            if let Some(rows) = block.get_mut(field).and_then(Value::as_array_mut) {
                for row in rows {
                    if let Some(cells) = row.as_array_mut() {
                        for cell in cells {
                            if let Some(blocks) =
                                cell.get_mut("blocks").and_then(Value::as_array_mut)
                            {
                                visit_blocks(blocks, visitor);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Enforces semantic bounds before constructing the larger typed block variants.
/// The JSON token depth has already been bounded by the shared reader.
pub(crate) fn validate_wire_limits(
    value: &Value,
    limits: &crate::limits::Limits,
) -> Result<(), crate::pipeline::PipelineError> {
    use crate::pipeline::PipelineError;
    let exceeded = |limit: &str| PipelineError::LimitExceeded {
        limit: limit.into(),
    };
    let mut stack = Vec::new();
    push_wire_nodes(&mut stack, value.get("body"), 0usize, true);
    push_wire_nodes(&mut stack, value.get("furniture"), 0usize, true);
    let mut blocks = 0usize;
    while let Some((node, depth, is_block)) = stack.pop() {
        if depth > usize::from(limits.max_nesting_depth) {
            return Err(exceeded("max_nesting_depth"));
        }
        if is_block {
            blocks += 1;
            if blocks > limits.max_blocks as usize {
                return Err(exceeded("max_blocks"));
            }
        }
        let kind = node.get("type").and_then(Value::as_str).unwrap_or("");
        let nested = if is_block {
            matches!(kind, "quote" | "footnote" | "list" | "table")
        } else {
            matches!(
                kind,
                "styled" | "emph" | "strong" | "strikeout" | "superscript" | "subscript" | "link"
            )
        };
        let child_depth = depth + usize::from(nested);
        if child_depth > usize::from(limits.max_nesting_depth) {
            return Err(exceeded("max_nesting_depth"));
        }
        if !is_block {
            if nested {
                push_wire_nodes(&mut stack, node.get("content"), child_depth, false);
            }
            continue;
        }
        match kind {
            "paragraph" | "heading" => {
                push_wire_nodes(&mut stack, node.get("content"), child_depth, false)
            }
            "figure" => push_wire_nodes(&mut stack, node.get("caption"), child_depth, false),
            "quote" | "footnote" => {
                push_wire_nodes(&mut stack, node.get("blocks"), child_depth, true)
            }
            "list" => {
                if let Some(items) = node.get("items").and_then(Value::as_array) {
                    for item in items {
                        push_wire_nodes(&mut stack, item.get("blocks"), child_depth, true);
                    }
                }
            }
            "table" => {
                push_wire_nodes(&mut stack, node.get("caption"), child_depth, false);
                push_wire_nodes(&mut stack, node.get("footnotes"), child_depth, false);
                for field in ["head", "body"] {
                    if let Some(rows) = node.get(field).and_then(Value::as_array) {
                        for row in rows {
                            if let Some(cells) = row.as_array() {
                                for cell in cells {
                                    push_wire_nodes(
                                        &mut stack,
                                        cell.get("blocks"),
                                        child_depth,
                                        true,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn push_wire_nodes<'a>(
    stack: &mut Vec<(&'a Value, usize, bool)>,
    values: Option<&'a Value>,
    depth: usize,
    block: bool,
) {
    if let Some(values) = values.and_then(Value::as_array) {
        stack.extend(values.iter().map(|value| (value, depth, block)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_payload_matches_canonical_legacy_serialization() {
        for header in [None, Some(true), Some(false)] {
            let block: Block = serde_json::from_value(serde_json::json!({
                "type":"table", "id":"table", "caption":[{"type":"styled", "content":[{"type":"text", "text":"caption"}]}],
                "columns":[{"align":"left"}], "head":[[{"rowspan":2,"colspan":1,"header":header,
                    "blocks":[{"type":"paragraph","id":"cell","content":[{"type":"text","text":"quotes \" \\ Tiếng Việt"}]}]}]],
                "body":[[{"rowspan":1,"colspan":1,"blocks":[{"type":"paragraph","id":"other","content":[{"type":"strong","content":[{"type":"text","text":"nested"}]}]}]}]],
                "footnotes":[{"type":"styled","content":[{"type":"footnote_ref","id":"note"}]}]
            })).unwrap();
            for block in [
                block.clone(),
                match block {
                    Block::Table {
                        id,
                        style,
                        direct,
                        columns,
                        head,
                        body,
                        ..
                    } => Block::Table {
                        id,
                        style,
                        direct,
                        columns,
                        head,
                        body,
                        caption: None,
                        footnotes: vec![],
                    },
                    _ => unreachable!(),
                },
            ] {
                let mut reference = serde_json::to_value(&block).unwrap();
                downgrade_block(&mut reference);
                assert_eq!(
                    semantic_payload(&block),
                    serde_json::to_vec(&reference).unwrap()
                );
            }
        }
    }

    #[test]
    fn plain_paragraph_payload_matches_canonical_legacy_serialization() {
        for text in [
            "plain",
            "quotes \" and \\ escapes\n",
            "Tiếng Việt",
            "\u{0000}\r\n",
        ] {
            let block = Block::Paragraph {
                id: super::super::BlockId::new("existing-id").unwrap(),
                style: Some(super::super::StyleId::new("named").unwrap()),
                direct: super::super::ParagraphStyle {
                    size_pt: Some(14.0),
                    ..Default::default()
                },
                content: vec![super::super::Inline::Text { text: text.into() }],
            };
            let mut reference = serde_json::to_value(&block).unwrap();
            downgrade_block(&mut reference);
            assert_eq!(
                semantic_payload(&block),
                serde_json::to_vec(&reference).unwrap()
            );
        }
    }
}
