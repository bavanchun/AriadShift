//! Parameters for listing, filling, creating, detecting and exchanging form fields.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::form_model::{
    ChoiceOption, FieldType, FormValue, MAX_FIELDS, MAX_FILL_VALUE_BYTES, MAX_IMPORT_RECORDS,
    MAX_NEW_FIELDS,
};
use crate::tools::PageRange;

/// Selects fields to list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsListParams {
    #[serde(default)]
    pub pages: PageRange,
    #[serde(default = "yes")]
    pub include_values: bool,
}

/// Values to apply to a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsFillParams {
    pub values: BTreeMap<String, FormValue>,
    #[serde(default)]
    pub flatten: bool,
    #[serde(default)]
    pub strict: bool,
}

impl FormsFillParams {
    /// Validate the size of each supplied field name and value.
    pub fn validate(&self) -> Result<(), String> {
        if self.values.len() > MAX_FIELDS {
            return Err("too many form values".into());
        }
        for (name, value) in &self.values {
            if name.is_empty()
                || name.len() > MAX_FILL_VALUE_BYTES
                || value_size(value) > MAX_FILL_VALUE_BYTES
            {
                return Err("invalid form name or value size".into());
            }
        }
        Ok(())
    }
}

fn value_size(value: &FormValue) -> usize {
    match value {
        FormValue::Text(s) => s.len(),
        FormValue::Multi(values) => values.iter().fold(0usize, |n, s| n.saturating_add(s.len())),
        FormValue::Bool(_) | FormValue::Null(()) => 0,
    }
}

/// Selects form fields to flatten.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsFlattenParams {
    #[serde(default)]
    pub pages: PageRange,
    #[serde(default)]
    pub fields: Option<Vec<String>>,
    #[serde(default)]
    pub include_annotations: bool,
}

/// A new field or radio-group widget, in PDF page coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewField {
    #[serde(rename = "type")]
    pub field_type: FieldType,
    pub name: String,
    pub page: u32,
    pub rect: [f64; 4],
    #[serde(default)]
    pub options: Option<Vec<ChoiceOption>>,
    #[serde(default)]
    pub default: Option<FormValue>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub tooltip: Option<String>,
    #[serde(default)]
    pub tab_order: Option<u32>,
    #[serde(default)]
    pub export_value: Option<String>,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub max_length: Option<u32>,
    #[serde(default)]
    pub font_size: f64,
    #[serde(default)]
    pub read_only: bool,
}

impl NewField {
    /// Reject malformed field specifications before mutating a document.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() || self.name.len() > MAX_FILL_VALUE_BYTES || self.name.contains('.')
        {
            return Err("field name must be nonempty and cannot contain a dot".into());
        }
        if self.page == 0
            || self.rect.iter().any(|n| !n.is_finite())
            || self.rect[0] >= self.rect[2]
            || self.rect[1] >= self.rect[3]
        {
            return Err("field requires a valid page and nonempty finite rectangle".into());
        }
        if self
            .default
            .as_ref()
            .is_some_and(|value| value_size(value) > MAX_FILL_VALUE_BYTES)
            || self
                .tooltip
                .as_ref()
                .is_some_and(|s| s.len() > MAX_FILL_VALUE_BYTES)
            || self
                .export_value
                .as_ref()
                .is_some_and(|s| s.len() > MAX_FILL_VALUE_BYTES)
            || self.options.as_ref().is_some_and(|options| {
                options.len() > MAX_FIELDS
                    || options.iter().any(|option| {
                        option.export.len() > MAX_FILL_VALUE_BYTES
                            || option.display.len() > MAX_FILL_VALUE_BYTES
                    })
            })
        {
            return Err("field value is too large".into());
        }
        if (self.multiline || self.max_length.is_some()) && self.field_type != FieldType::Text {
            return Err("multiline and max_length apply only to text fields".into());
        }
        if self.options.is_some() && !matches!(self.field_type, FieldType::Combo | FieldType::List)
        {
            return Err("options apply only to choice fields".into());
        }
        if let Some(default) = &self.default {
            let valid = match (self.field_type, default) {
                (_, FormValue::Null(())) => true,
                (FieldType::Text, FormValue::Text(_)) => true,
                (FieldType::Checkbox, FormValue::Bool(_)) => true,
                (FieldType::Checkbox, FormValue::Text(value)) => {
                    value == "Off" || self.export_value.as_ref() == Some(value)
                }
                (FieldType::Radio, FormValue::Text(_)) => true,
                (FieldType::Combo | FieldType::List, FormValue::Text(value)) => self
                    .options
                    .as_ref()
                    .is_some_and(|options| options.iter().any(|option| &option.export == value)),
                _ => false,
            };
            if !valid {
                return Err("default value does not match the field type or options".into());
            }
        }
        if !self.font_size.is_finite() || self.font_size < 0.0 {
            return Err("font_size must be finite and nonnegative".into());
        }
        if matches!(self.field_type, FieldType::Checkbox | FieldType::Radio)
            && self
                .export_value
                .as_deref()
                .is_none_or(|value| value.is_empty() || value == "Off")
        {
            return Err("checkbox and radio fields require a non-Off export_value".into());
        }
        Ok(())
    }
}

/// Fields to add to an AcroForm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsCreateParams {
    pub fields: Vec<NewField>,
    #[serde(default)]
    pub replace_existing: bool,
}
impl FormsCreateParams {
    /// Validate field count and every proposed field.
    pub fn validate(&self) -> Result<(), String> {
        if self.fields.is_empty() || self.fields.len() > MAX_NEW_FIELDS {
            return Err("invalid number of new fields".into());
        }
        self.fields.iter().try_for_each(NewField::validate)
    }
}

/// Detection strategy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FormsDetectMode {
    #[default]
    Auto,
    Vector,
    Raster,
}

/// Detection selection and threshold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsDetectParams {
    #[serde(default)]
    pub pages: PageRange,
    #[serde(default)]
    pub mode: FormsDetectMode,
    #[serde(default = "half")]
    pub min_confidence: f64,
}

impl FormsDetectParams {
    /// Validate the detection threshold.
    pub fn validate(&self) -> Result<(), String> {
        if !self.min_confidence.is_finite() || !(0.0..=1.0).contains(&self.min_confidence) {
            return Err("min_confidence must be between zero and one".into());
        }
        Ok(())
    }
}

/// A detected field proposal requiring user confirmation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetectedField {
    pub proposal: NewField,
    pub label: Option<String>,
    pub confidence: f64,
    pub reason: String,
}

/// Treatment of empty cells during import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmptyCell {
    #[default]
    Skip,
    Clear,
}

/// Form-data import options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsImportParams {
    #[serde(default)]
    pub flatten: bool,
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub empty: EmptyCell,
    #[serde(default)]
    pub delimiter: Option<char>,
    #[serde(default)]
    pub name_column: Option<String>,
    #[serde(default = "default_max_records")]
    pub max_records: usize,
}

impl FormsImportParams {
    /// Validate the batch limit and optional single-byte CSV delimiter.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_records == 0 || self.max_records > MAX_IMPORT_RECORDS {
            return Err("max_records is outside the allowed range".into());
        }
        if self
            .delimiter
            .is_some_and(|c| !c.is_ascii() || matches!(c, '\r' | '\n' | '"' | '\0'))
        {
            return Err("invalid CSV delimiter".into());
        }
        Ok(())
    }
}

/// Form-data export format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FormsExportFormat {
    Fdf,
    Xfdf,
    #[default]
    Json,
    Csv,
}

/// Form-data export options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsExportParams {
    #[serde(default)]
    pub format: FormsExportFormat,
    #[serde(default)]
    pub pages: PageRange,
}

fn yes() -> bool {
    true
}
fn half() -> f64 {
    0.5
}
fn default_max_records() -> usize {
    1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_field_defaults_against_type_and_options() {
        let field = |value| serde_json::from_value::<NewField>(value).unwrap();
        assert!(field(json!({"type":"text","name":"x","page":1,"rect":[0,0,10,10],"default":true})).validate().is_err());
        assert!(field(json!({"type":"checkbox","name":"x","page":1,"rect":[0,0,10,10],"export_value":"Approved","default":"Yes"})).validate().is_err());
        assert!(field(json!({"type":"checkbox","name":"x","page":1,"rect":[0,0,10,10],"export_value":"Approved","default":"Approved"})).validate().is_ok());
        assert!(field(json!({"type":"combo","name":"x","page":1,"rect":[0,0,10,10],"options":[{"export":"a","display":"A"}],"default":"b"})).validate().is_err());
    }

    #[test]
    fn defaults_and_validation() {
        let list: FormsListParams = serde_json::from_value(json!({})).unwrap();
        assert!(list.include_values);
        assert_eq!(list.pages.as_str(), "all");
        let import: FormsImportParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(import.max_records, 1000);
        let mut field: NewField = serde_json::from_value(
            json!({"type":"text", "name":"name", "page":1, "rect":[0,0,100,20]}),
        )
        .unwrap();
        assert!(field.validate().is_ok());
        field.name = "a.b".into();
        assert!(field.validate().is_err());
        assert!(serde_json::from_value::<FormsFlattenParams>(json!({"extra": true})).is_err());
    }
}
