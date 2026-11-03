//! Shared values for drawing on PDF pages.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Position of a mark in the displayed page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    /// Upper left.
    TopLeft,
    /// Upper centre.
    TopCenter,
    /// Upper right.
    TopRight,
    /// Middle left.
    MiddleLeft,
    /// Centre of the page.
    #[default]
    Center,
    /// Middle right.
    MiddleRight,
    /// Lower left.
    BottomLeft,
    /// Lower centre.
    BottomCenter,
    /// Lower right.
    BottomRight,
}

/// Bundled font face.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FontFace {
    /// Regular Libertinus Serif.
    #[default]
    Serif,
    /// Bold Libertinus Serif.
    SerifBold,
}

/// Format for an expanded date token.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DateFormat {
    /// Year-month-day.
    #[default]
    Iso,
    /// Day/month/year.
    Dmy,
    /// Month/day/year.
    Mdy,
}

/// An RGB colour expressed as `#RRGGBB`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct HexColor(#[schemars(regex(pattern = "^#[0-9A-Fa-f]{6}$"))] String);

impl HexColor {
    /// Returns the validated colour string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
            return Err(serde::de::Error::custom("colour must be #RRGGBB"));
        }
        Ok(Self(value))
    }
}

/// Opacity in the inclusive range from zero to one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Opacity(#[schemars(range(min = 0.0, max = 1.0))] f64);

impl Opacity {
    /// Returns the opacity value.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Opacity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(serde::de::Error::custom("opacity must be between 0 and 1"));
        }
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overlay_common_values_validate() {
        assert_eq!(
            serde_json::from_value::<Anchor>(json!("bottom-center")).unwrap(),
            Anchor::BottomCenter
        );
        assert!(serde_json::from_value::<Anchor>(json!("bottom_center")).is_err());
        assert_eq!(
            serde_json::from_value::<FontFace>(json!("serif-bold")).unwrap(),
            FontFace::SerifBold
        );
        assert!(serde_json::from_value::<FontFace>(json!("mono")).is_err());
        assert_eq!(
            serde_json::from_value::<DateFormat>(json!("dmy")).unwrap(),
            DateFormat::Dmy
        );
        assert_eq!(
            serde_json::from_value::<HexColor>(json!("#ab12EF"))
                .unwrap()
                .as_str(),
            "#ab12EF"
        );
        for invalid in ["#123", "123456", "#gg0000", "#1234567", "#１２３"] {
            assert!(serde_json::from_value::<HexColor>(json!(invalid)).is_err());
        }
        for valid in [0.0, 0.5, 1.0] {
            assert_eq!(
                serde_json::from_value::<Opacity>(json!(valid))
                    .unwrap()
                    .value(),
                valid
            );
        }
        for invalid in [-0.1, 1.1] {
            assert!(serde_json::from_value::<Opacity>(json!(invalid)).is_err());
        }
        let schema = schemars::schema_for!(HexColor);
        assert_eq!(
            serde_json::to_value(schema).unwrap()["pattern"],
            "^#[0-9A-Fa-f]{6}$"
        );
    }
}
