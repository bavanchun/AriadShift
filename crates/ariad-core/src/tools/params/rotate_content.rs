//! Parameter types for the `pdf.rotate_content` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::page_range::PageRange;

const fn default_expand() -> bool {
    true
}

/// Parameters for `pdf.rotate_content`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RotateContentParams {
    /// 1-based pages whose content streams should be rotated (`"all"` by default).
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Arbitrary rotation angle in degrees (`-360.0..=360.0`).
    #[serde(deserialize_with = "deserialize_angle_deg")]
    #[schemars(range(min = -360.0, max = 360.0))]
    pub angle_deg: f32,
    /// Whether to expand the page `/MediaBox` so rotated corners are not clipped (`true` by default).
    #[serde(default = "default_expand")]
    pub expand: bool,
}

fn deserialize_angle_deg<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    let angle = f32::deserialize(deserializer)?;
    if !angle.is_finite() || !(-360.0..=360.0).contains(&angle) {
        return Err(serde::de::Error::custom(
            "angle_deg must be a finite number in [-360.0, 360.0]",
        ));
    }
    Ok(angle)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rotate_content_params_defaults_unknown_field_and_valid_invalid_json() {
        let valid: RotateContentParams =
            serde_json::from_value(json!({"angle_deg": 15.5})).unwrap();
        assert_eq!(valid.pages.as_str(), "all");
        assert!((valid.angle_deg - 15.5).abs() < 1e-5);
        assert!(valid.expand);

        let custom: RotateContentParams = serde_json::from_value(json!({
            "pages": "1-3",
            "angle_deg": -45.0,
            "expand": false
        }))
        .unwrap();
        assert_eq!(custom.pages.as_str(), "1-3");
        assert!(!custom.expand);

        assert!(serde_json::from_value::<RotateContentParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<RotateContentParams>(json!({"angle_deg": 500.0})).is_err()
        );
        assert!(
            serde_json::from_value::<RotateContentParams>(json!({"angle_deg": 30.0, "extra": 1}))
                .is_err()
        );
    }
}
