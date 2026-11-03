//! Shared, engine-independent AcroForm data types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Maximum number of fields accepted when creating a form.
pub const MAX_NEW_FIELDS: usize = 2_000;
/// Maximum number of fields traversed in an existing document.
pub const MAX_FIELDS: usize = 50_000;
/// Maximum depth of the field hierarchy.
pub const MAX_FIELD_DEPTH: usize = 32;
/// Maximum size of a single value in bytes.
pub const MAX_FILL_VALUE_BYTES: usize = 65_536;
/// Maximum number of records accepted by batch import.
pub const MAX_IMPORT_RECORDS: usize = 10_000;

/// A form value; null requests that the existing value be cleared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FormValue {
    Text(String),
    Bool(bool),
    Multi(Vec<String>),
    Null(()),
}

/// PDF form field types exposed by the tool API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Checkbox,
    Radio,
    Combo,
    List,
    Button,
    Signature,
}

/// Interpreted PDF field flags.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FieldFlags {
    pub read_only: bool,
    pub required: bool,
    pub no_export: bool,
    pub multiline: bool,
    pub password: bool,
    pub comb: bool,
    pub editable: bool,
    pub multi_select: bool,
}

/// The stored and displayed values of a choice field option.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChoiceOption {
    pub export: String,
    pub display: String,
}

/// A widget's location and optional non-Off appearance state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WidgetInfo {
    pub page: u32,
    pub rect: [f64; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_state: Option<String>,
}

/// Static action metadata. Actions are never executed by these tools.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ActionSummary {
    pub has_javascript: bool,
    pub kinds: Vec<String>,
}

/// A field and its inherited properties as read from a PDF.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: Option<FieldType>,
    pub value: FormValue,
    pub default_value: FormValue,
    pub options: Vec<ChoiceOption>,
    pub flags: FieldFlags,
    pub max_length: Option<u32>,
    pub page: u32,
    pub rect: [f64; 4],
    pub widgets: Vec<WidgetInfo>,
    pub tooltip: Option<String>,
    pub tab_order: Option<u32>,
    pub signed: bool,
    pub actions: ActionSummary,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn form_values_round_trip_without_conflating_null_and_false() {
        for value in [
            json!(null),
            json!(false),
            json!("Tiếng Việt"),
            json!(["a", "b"]),
        ] {
            let parsed: FormValue = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(parsed).unwrap(), value);
        }
    }

    #[test]
    fn flags_default_to_false_and_reject_unknown_keys() {
        let flags: FieldFlags = serde_json::from_value(json!({"required": true})).unwrap();
        assert!(flags.required);
        assert!(!flags.read_only);
        assert!(serde_json::from_value::<FieldFlags>(json!({"unknown": true})).is_err());
    }
}
