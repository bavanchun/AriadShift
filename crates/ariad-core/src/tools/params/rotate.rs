//! Parameter types for the `pdf.rotate` tool.

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::page_range::PageRange;

/// Parameters for `pdf.rotate`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RotateParams {
    /// 1-based pages to rotate (`"all"` by default; duplicate pages are rejected).
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Clockwise rotation delta in degrees (`90`, `180`, `270`, or `-90`).
    #[serde(deserialize_with = "deserialize_quarter_turn")]
    #[schemars(schema_with = "quarter_turn_schema")]
    pub angle: i32,
}

fn deserialize_quarter_turn<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let angle = i32::deserialize(deserializer)?;
    match angle {
        90 | 180 | 270 | -90 => Ok(angle),
        other => Err(serde::de::Error::custom(format!(
            "invalid rotation angle {other}: expected 90, 180, 270, or -90"
        ))),
    }
}

fn quarter_turn_schema(_gen: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "description": "Clockwise rotation delta in degrees (`90`, `180`, `270`, or `-90`).",
        "type": "integer",
        "enum": [90, 180, 270, -90],
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rotate_params_defaults_unknown_field_and_valid_invalid_json() {
        let valid_default_pages: RotateParams =
            serde_json::from_value(json!({"angle": 90})).unwrap();
        assert_eq!(valid_default_pages.pages.as_str(), "all");
        assert_eq!(valid_default_pages.angle, 90);

        for angle in [90, 180, 270, -90] {
            let parsed: RotateParams =
                serde_json::from_value(json!({"pages": "1-3", "angle": angle})).unwrap();
            assert_eq!(parsed.angle, angle);
        }

        assert!(serde_json::from_value::<RotateParams>(json!({})).is_err());
        assert!(serde_json::from_value::<RotateParams>(json!({"angle": 45})).is_err());
        assert!(serde_json::from_value::<RotateParams>(json!({"angle": 90, "extra": 1})).is_err());
    }
}
