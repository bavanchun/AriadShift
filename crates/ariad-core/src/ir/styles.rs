//! Shared style contracts and bounded cascade resolution.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Named style identity, independent of block identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StyleId(String);
impl StyleId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if !valid_identifier(&value) {
            return Err("invalid style id".into());
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for StyleId {
    type Error = String;
    fn try_from(v: String) -> Result<Self, String> {
        Self::new(v)
    }
}
impl From<StyleId> for String {
    fn from(v: StyleId) -> Self {
        v.0
    }
}
pub(super) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FontRef {
    pub family: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<String>,
}
impl FontRef {
    fn validate(&self) -> Result<(), String> {
        if self.family.trim().is_empty() || self.fallback.iter().any(|v| v.trim().is_empty()) {
            return Err("font family and fallbacks must be nonempty".into());
        }
        Ok(())
    }
}

/// Canonical sRGB colour, encoded as lowercase #rrggbb.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Colour(String);
impl Colour {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.len() != 7
            || !value.starts_with('#')
            || !value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        {
            return Err("colour must be #rrggbb".into());
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Colour {
    type Error = String;
    fn try_from(v: String) -> Result<Self, String> {
        Self::new(v)
    }
}
impl From<Colour> for String {
    fn from(v: Colour) -> Self {
        v.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParagraphAlignment {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParagraphStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<StyleId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<FontRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("exclusiveMinimum" = 0.0))]
    pub size_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("exclusiveMinimum" = 0.0))]
    pub line_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0))]
    pub space_before_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0))]
    pub space_after_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<ParagraphAlignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_first_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_left_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_right_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_break_before: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Colour>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Colour>,
}
impl ParagraphStyle {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn overlay(&mut self, other: &Self) {
        if other.based_on.is_some() {
            self.based_on.clone_from(&other.based_on);
        }
        if other.font.is_some() {
            self.font.clone_from(&other.font);
        }
        if other.size_pt.is_some() {
            self.size_pt.clone_from(&other.size_pt);
        }
        if other.line_height.is_some() {
            self.line_height.clone_from(&other.line_height);
        }
        if other.space_before_pt.is_some() {
            self.space_before_pt.clone_from(&other.space_before_pt);
        }
        if other.space_after_pt.is_some() {
            self.space_after_pt.clone_from(&other.space_after_pt);
        }
        if other.align.is_some() {
            self.align.clone_from(&other.align);
        }
        if other.indent_first_pt.is_some() {
            self.indent_first_pt.clone_from(&other.indent_first_pt);
        }
        if other.indent_left_pt.is_some() {
            self.indent_left_pt.clone_from(&other.indent_left_pt);
        }
        if other.indent_right_pt.is_some() {
            self.indent_right_pt.clone_from(&other.indent_right_pt);
        }
        if other.keep_with_next.is_some() {
            self.keep_with_next.clone_from(&other.keep_with_next);
        }
        if other.page_break_before.is_some() {
            self.page_break_before.clone_from(&other.page_break_before);
        }
        if other.color.is_some() {
            self.color.clone_from(&other.color);
        }
        if other.background.is_some() {
            self.background.clone_from(&other.background);
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = &self.font {
            v.validate()?;
        }
        if self.size_pt.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err("invalid size_pt".into());
        }
        if self.line_height.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err("invalid line_height".into());
        }
        if self
            .space_before_pt
            .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err("invalid space_before_pt".into());
        }
        if self
            .space_after_pt
            .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err("invalid space_after_pt".into());
        }
        if self.indent_first_pt.is_some_and(|v| !v.is_finite()) {
            return Err("invalid indent_first_pt".into());
        }
        if self.indent_left_pt.is_some_and(|v| !v.is_finite()) {
            return Err("invalid indent_left_pt".into());
        }
        if self.indent_right_pt.is_some_and(|v| !v.is_finite()) {
            return Err("invalid indent_right_pt".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<FontRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("exclusiveMinimum" = 0.0))]
    pub size_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 1000))]
    pub weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Colour>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Colour>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_shift: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub small_caps: Option<bool>,
}
impl CharacterStyle {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn overlay(&mut self, other: &Self) {
        if other.font.is_some() {
            self.font.clone_from(&other.font);
        }
        if other.size_pt.is_some() {
            self.size_pt.clone_from(&other.size_pt);
        }
        if other.weight.is_some() {
            self.weight.clone_from(&other.weight);
        }
        if other.italic.is_some() {
            self.italic.clone_from(&other.italic);
        }
        if other.underline.is_some() {
            self.underline.clone_from(&other.underline);
        }
        if other.strike.is_some() {
            self.strike.clone_from(&other.strike);
        }
        if other.color.is_some() {
            self.color.clone_from(&other.color);
        }
        if other.highlight.is_some() {
            self.highlight.clone_from(&other.highlight);
        }
        if other.baseline_shift.is_some() {
            self.baseline_shift.clone_from(&other.baseline_shift);
        }
        if other.letter_spacing.is_some() {
            self.letter_spacing.clone_from(&other.letter_spacing);
        }
        if other.small_caps.is_some() {
            self.small_caps.clone_from(&other.small_caps);
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = &self.font {
            v.validate()?;
        }
        if self.size_pt.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err("invalid size_pt".into());
        }
        if self.baseline_shift.is_some_and(|v| !v.is_finite()) {
            return Err("invalid baseline_shift".into());
        }
        if self.letter_spacing.is_some_and(|v| !v.is_finite()) {
            return Err("invalid letter_spacing".into());
        }
        if self.weight.is_some_and(|v| !(1..=1000).contains(&v)) {
            return Err("weight must be between 1 and 1000".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableStyle {
    #[serde(default, skip_serializing_if = "ParagraphStyle::is_empty")]
    pub paragraph: ParagraphStyle,
    #[serde(default, skip_serializing_if = "CharacterStyle::is_empty")]
    pub character: CharacterStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Colour>,
}
impl TableStyle {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    fn validate(&self) -> Result<(), String> {
        self.paragraph.validate()?;
        self.character.validate()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleDefaults {
    #[serde(default, skip_serializing_if = "ParagraphStyle::is_empty")]
    pub paragraph: ParagraphStyle,
    #[serde(default, skip_serializing_if = "CharacterStyle::is_empty")]
    pub character: CharacterStyle,
    #[serde(default, skip_serializing_if = "TableStyle::is_empty")]
    pub table: TableStyle,
}
impl StyleDefaults {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleSheet {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub paragraph: BTreeMap<StyleId, ParagraphStyle>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub character: BTreeMap<StyleId, CharacterStyle>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub table: BTreeMap<StyleId, TableStyle>,
    #[serde(default, skip_serializing_if = "StyleDefaults::is_empty")]
    pub defaults: StyleDefaults,
}
impl StyleSheet {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn resolve_paragraph(
        &self,
        named: Option<&StyleId>,
        direct: &ParagraphStyle,
        max_depth: u16,
    ) -> Result<ParagraphStyle, String> {
        let mut result = ParagraphStyle::default();
        self.apply_paragraph(
            &mut result,
            self.defaults.paragraph.based_on.as_ref(),
            max_depth,
        )?;
        result.overlay(&self.defaults.paragraph);
        self.apply_paragraph(&mut result, named, max_depth)?;
        self.apply_paragraph(&mut result, direct.based_on.as_ref(), max_depth)?;
        result.overlay(direct);
        result.based_on = None;
        result.validate()?;
        Ok(result)
    }
    fn apply_paragraph(
        &self,
        result: &mut ParagraphStyle,
        named: Option<&StyleId>,
        max_depth: u16,
    ) -> Result<(), String> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = named;
        while let Some(id) = current {
            if chain.len() >= usize::from(max_depth) {
                return Err("style inheritance exceeds nesting limit".into());
            }
            if !seen.insert(id) {
                return Err("style inheritance cycle".into());
            }
            let value = self
                .paragraph
                .get(id)
                .ok_or("dangling paragraph style reference")?;
            chain.push(value);
            current = value.based_on.as_ref();
        }
        for value in chain.into_iter().rev() {
            result.overlay(value);
        }
        Ok(())
    }
    pub fn resolve_character(
        &self,
        named: Option<&StyleId>,
        direct: &CharacterStyle,
        parent: Option<&CharacterStyle>,
    ) -> Result<CharacterStyle, String> {
        let mut result = self.defaults.character.clone();
        if let Some(parent) = parent {
            result.overlay(parent);
        }
        if let Some(id) = named {
            result.overlay(
                self.character
                    .get(id)
                    .ok_or("dangling character style reference")?,
            );
        }
        result.overlay(direct);
        result.validate()?;
        Ok(result)
    }
    pub fn resolve_table(
        &self,
        named: Option<&StyleId>,
        direct: &ParagraphStyle,
        max_depth: u16,
    ) -> Result<TableStyle, String> {
        let mut result = TableStyle {
            paragraph: self.resolve_paragraph(None, &self.defaults.table.paragraph, max_depth)?,
            character: self.resolve_character(None, &self.defaults.table.character, None)?,
            background: self.defaults.table.background.clone(),
        };
        if let Some(id) = named {
            let value = self.table.get(id).ok_or("dangling table style reference")?;
            self.apply_paragraph(
                &mut result.paragraph,
                value.paragraph.based_on.as_ref(),
                max_depth,
            )?;
            result.paragraph.overlay(&value.paragraph);
            result.character.overlay(&value.character);
            if value.background.is_some() {
                result.background.clone_from(&value.background);
            }
        }
        self.apply_paragraph(&mut result.paragraph, direct.based_on.as_ref(), max_depth)?;
        result.paragraph.overlay(direct);
        result.paragraph.based_on = None;
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self, max_depth: u16, max_styles: usize) -> Result<(), String> {
        if self
            .paragraph
            .len()
            .saturating_add(self.character.len())
            .saturating_add(self.table.len())
            > max_styles
        {
            return Err("style count exceeds block limit".into());
        }
        self.defaults.paragraph.validate()?;
        self.defaults.character.validate()?;
        self.defaults.table.validate()?;
        self.resolve_paragraph(None, &self.defaults.paragraph, max_depth)?;
        self.resolve_paragraph(None, &self.defaults.table.paragraph, max_depth)?;
        for (id, value) in &self.paragraph {
            value.validate()?;
            self.resolve_paragraph(Some(id), &ParagraphStyle::default(), max_depth)?;
        }
        for value in self.character.values() {
            value.validate()?;
        }
        for (id, value) in &self.table {
            value.validate()?;
            self.resolve_table(Some(id), &ParagraphStyle::default(), max_depth)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontEmbedding {
    Allowed,
    Restricted,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FontAsset {
    pub family: String,
    pub embedding: FontEmbedding,
}
impl FontAsset {
    pub fn can_embed(&self) -> bool {
        self.embedding == FontEmbedding::Allowed
    }
}

impl JsonSchema for StyleId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "StyleId".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "pattern":"^[A-Za-z][A-Za-z0-9_.-]{0,127}$"})
    }
}

impl JsonSchema for Colour {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Colour".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "pattern":"^#[0-9a-f]{6}$"})
    }
}
