//! Stable block identities, section setup and geometry references.
use super::{Block, BlockPosition, Document, ParagraphStyle, StyleId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The empty default exists only while an importer constructs a document.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BlockId(String);
impl BlockId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if !super::styles::valid_identifier(&value) {
            return Err("invalid block id".into());
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Temporary positional key used only before import finalization.
    #[doc(hidden)]
    pub(crate) fn import_path(path: impl Into<String>) -> Self {
        Self(path.into())
    }
}
impl TryFrom<String> for BlockId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        Self::new(value)
    }
}
impl From<BlockId> for String {
    fn from(value: BlockId) -> Self {
        value.0
    }
}
impl std::borrow::Borrow<str> for BlockId {
    fn borrow(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
pub type SourceMap = BTreeMap<BlockId, Vec<BlockPosition>>;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageSize {
    pub width_pt: f64,
    pub height_pt: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Margins {
    pub top_pt: f64,
    pub right_pt: f64,
    pub bottom_pt: f64,
    pub left_pt: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Section {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<BlockId>,
    pub page_size: PageSize,
    pub margins: Margins,
    pub columns: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<BlockId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<BlockId>,
}

impl Block {
    pub fn id(&self) -> &BlockId {
        match self {
            Self::Heading { id, .. }
            | Self::Paragraph { id, .. }
            | Self::Code { id, .. }
            | Self::Math { id, .. }
            | Self::Quote { id, .. }
            | Self::PageBreak { id, .. }
            | Self::List { id, .. }
            | Self::Table { id, .. }
            | Self::Figure { id, .. }
            | Self::Footnote { id, .. }
            | Self::Raw { id, .. } => id,
        }
    }
    pub fn id_mut(&mut self) -> &mut BlockId {
        match self {
            Self::Heading { id, .. }
            | Self::Paragraph { id, .. }
            | Self::Code { id, .. }
            | Self::Math { id, .. }
            | Self::Quote { id, .. }
            | Self::PageBreak { id, .. }
            | Self::List { id, .. }
            | Self::Table { id, .. }
            | Self::Figure { id, .. }
            | Self::Footnote { id, .. }
            | Self::Raw { id, .. } => id,
        }
    }
    pub fn style(&self) -> &Option<StyleId> {
        match self {
            Self::Heading { style, .. }
            | Self::Paragraph { style, .. }
            | Self::Code { style, .. }
            | Self::Math { style, .. }
            | Self::Quote { style, .. }
            | Self::PageBreak { style, .. }
            | Self::List { style, .. }
            | Self::Table { style, .. }
            | Self::Figure { style, .. }
            | Self::Footnote { style, .. }
            | Self::Raw { style, .. } => style,
        }
    }
    pub fn direct(&self) -> &ParagraphStyle {
        match self {
            Self::Heading { direct, .. }
            | Self::Paragraph { direct, .. }
            | Self::Code { direct, .. }
            | Self::Math { direct, .. }
            | Self::Quote { direct, .. }
            | Self::PageBreak { direct, .. }
            | Self::List { direct, .. }
            | Self::Table { direct, .. }
            | Self::Figure { direct, .. }
            | Self::Footnote { direct, .. }
            | Self::Raw { direct, .. } => direct,
        }
    }
}

/// Iterates over every semantic block with its legacy AST path, without recursion.
pub fn blocks_with_paths(document: &Document) -> Vec<(String, &Block)> {
    let mut result = Vec::new();
    let mut stack: Vec<_> = document
        .body
        .iter()
        .enumerate()
        .map(|(i, b)| (format!("body/{i}"), b))
        .chain(
            document
                .furniture
                .iter()
                .enumerate()
                .map(|(i, b)| (format!("furniture/{i}"), b)),
        )
        .collect();
    while let Some((path, block)) = stack.pop() {
        match block {
            Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
                stack.extend(
                    blocks
                        .iter()
                        .enumerate()
                        .map(|(i, b)| (format!("{path}/blocks/{i}"), b)),
                );
            }
            Block::List { items, .. } => {
                for (i, item) in items.iter().enumerate() {
                    stack.extend(
                        item.blocks
                            .iter()
                            .enumerate()
                            .map(|(j, b)| (format!("{path}/items/{i}/blocks/{j}"), b)),
                    );
                }
            }
            Block::Table { head, body, .. } => {
                for (field, rows) in [("head", head), ("body", body)] {
                    for (i, row) in rows.iter().enumerate() {
                        for (j, cell) in row.iter().enumerate() {
                            stack.extend(cell.blocks.iter().enumerate().map(|(k, b)| {
                                (format!("{path}/{field}/{i}/cells/{j}/blocks/{k}"), b)
                            }));
                        }
                    }
                }
            }
            _ => {}
        }
        result.push((path, block));
    }
    result
}
impl Document {
    /// Assigns only unassigned IDs and migrates positional geometry after the full AST exists.
    /// Call after bounded import construction, never to repair a serialized v2 document.
    pub fn finalize_import(mut self) -> Self {
        self.assign_block_ids();
        self
    }
    pub fn assign_block_ids(&mut self) {
        let mut used = BTreeSet::new();
        let mut pending: Vec<_> = self.body.iter().chain(&self.furniture).collect();
        while let Some(block) = pending.pop() {
            if !block.id().0.is_empty() {
                used.insert(block.id().clone());
            }
            match block {
                Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
                    pending.extend(blocks)
                }
                Block::List { items, .. } => {
                    for item in items {
                        pending.extend(&item.blocks);
                    }
                }
                Block::Table { head, body, .. } => {
                    for cell in head.iter().chain(body).flatten() {
                        pending.extend(&cell.blocks);
                    }
                }
                _ => {}
            }
        }
        let needs_mapping = self
            .layout
            .as_ref()
            .is_some_and(|layout| !layout.blocks.is_empty())
            || !self.source_map.is_empty();
        let mut mapping = BTreeMap::new();
        let mut stack: Vec<_> = self
            .body
            .iter_mut()
            .enumerate()
            .map(|(i, b)| (format!("body/{i}"), b))
            .chain(
                self.furniture
                    .iter_mut()
                    .enumerate()
                    .map(|(i, b)| (format!("furniture/{i}"), b)),
            )
            .collect();
        while let Some((path, block)) = stack.pop() {
            if block.id().0.is_empty() {
                let payload = super::version::semantic_payload(block);
                let mut attempt = 0u64;
                loop {
                    let mut hash = Sha256::new();
                    hash.update(b"ariad-ir/2:block\0");
                    hash.update(path.as_bytes());
                    hash.update([0]);
                    hash.update(&payload);
                    if attempt != 0 {
                        hash.update(attempt.to_le_bytes());
                    }
                    let id = BlockId(format!("b{}", hex::encode(hash.finalize())));
                    if used.insert(id.clone()) {
                        *block.id_mut() = id;
                        break;
                    }
                    attempt += 1;
                }
            }
            if needs_mapping {
                mapping.insert(path.clone(), block.id().clone());
            }
            match block {
                Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
                    stack.extend(
                        blocks
                            .iter_mut()
                            .enumerate()
                            .map(|(i, b)| (format!("{path}/blocks/{i}"), b)),
                    );
                }
                Block::List { items, .. } => {
                    for (i, item) in items.iter_mut().enumerate() {
                        stack.extend(
                            item.blocks
                                .iter_mut()
                                .enumerate()
                                .map(|(j, b)| (format!("{path}/items/{i}/blocks/{j}"), b)),
                        );
                    }
                }
                Block::Table { head, body, .. } => {
                    for (field, rows) in [("head", head), ("body", body)] {
                        for (i, row) in rows.iter_mut().enumerate() {
                            for (j, cell) in row.iter_mut().enumerate() {
                                stack.extend(cell.blocks.iter_mut().enumerate().map(|(k, b)| {
                                    (format!("{path}/{field}/{i}/cells/{j}/blocks/{k}"), b)
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(layout) = &mut self.layout {
            layout.blocks = std::mem::take(&mut layout.blocks)
                .into_iter()
                .map(|(key, value)| (mapping.get(key.as_str()).cloned().unwrap_or(key), value))
                .collect();
        }
        self.source_map = std::mem::take(&mut self.source_map)
            .into_iter()
            .map(|(key, value)| (mapping.get(key.as_str()).cloned().unwrap_or(key), value))
            .collect();
        self.version = super::IR_VERSION.to_owned();
    }
    pub(crate) fn validate_geometry(&self) -> Result<(), String> {
        let blocks = blocks_with_paths(self);
        let ids: BTreeSet<_> = blocks.iter().map(|(_, b)| b.id()).collect();
        if ids.len() != blocks.len()
            || ids
                .iter()
                .any(|id| !super::styles::valid_identifier(id.as_str()))
        {
            return Err("block IDs must be assigned, valid and unique".into());
        }
        let check_position = |id: &BlockId, pos: &BlockPosition| -> Result<(), String> {
            if !ids.contains(id) {
                return Err("dangling geometry block ID".into());
            }
            if pos.page_no == 0
                || self.layout.as_ref().is_some_and(|layout| {
                    !layout.pages.is_empty()
                        && !layout.pages.iter().any(|page| page.page_no == pos.page_no)
                })
            {
                return Err("geometry references missing page".into());
            }
            let b = pos.bbox;
            if ![b.left, b.top, b.right, b.bottom]
                .iter()
                .all(|v| v.is_finite())
                || b.right < b.left
                || b.bottom < b.top
            {
                return Err("invalid block geometry".into());
            }
            Ok(())
        };
        if let Some(layout) = &self.layout {
            let mut pages = BTreeSet::new();
            for page in &layout.pages {
                if page.page_no == 0
                    || !pages.insert(page.page_no)
                    || !page.width.is_finite()
                    || !page.height.is_finite()
                    || page.width <= 0.0
                    || page.height <= 0.0
                {
                    return Err("invalid page geometry".into());
                }
            }
            for (id, pos) in &layout.blocks {
                check_position(id, pos)?;
            }
        }
        for (id, positions) in &self.source_map {
            if positions.is_empty() {
                return Err("empty source-map fragments".into());
            }
            for pos in positions {
                check_position(id, pos)?;
            }
        }
        let body: BTreeMap<_, _> = self
            .body
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id(), i))
            .collect();
        let furniture: BTreeSet<_> = blocks
            .iter()
            .filter(|(p, _)| p.starts_with("furniture/"))
            .map(|(_, b)| b.id())
            .collect();
        let mut previous = None;
        for (i, section) in self.sections.iter().enumerate() {
            let start = match &section.start {
                Some(id) => *body
                    .get(id)
                    .ok_or("section start must reference a body block")?,
                None if i == 0 => 0,
                None => return Err("later sections require a start ID".into()),
            };
            if previous.is_some_and(|v| v >= start) {
                return Err("section starts must follow document order".into());
            }
            previous = Some(start);
            for id in section.header.iter().chain(section.footer.iter()) {
                if !furniture.contains(id) {
                    return Err("section header/footer must reference furniture".into());
                }
            }
            let p = section.page_size;
            let m = section.margins;
            if ![
                p.width_pt,
                p.height_pt,
                m.top_pt,
                m.right_pt,
                m.bottom_pt,
                m.left_pt,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
                || p.width_pt <= m.left_pt + m.right_pt
                || p.height_pt <= m.top_pt + m.bottom_pt
                || section.columns == 0
            {
                return Err("invalid section page setup".into());
            }
        }
        Ok(())
    }
}

impl JsonSchema for BlockId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BlockId".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "pattern":"^[A-Za-z][A-Za-z0-9_.-]{0,127}$"})
    }
}
