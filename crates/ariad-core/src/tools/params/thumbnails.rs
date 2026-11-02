//! Parameter type for the `pdf.thumbnails` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ParamsError, check_range};
use crate::tools::PageRange;

/// Smallest accepted longest-edge size of a thumbnail in pixels.
pub const MIN_THUMBNAIL_PX: u32 = 64;
/// Largest accepted longest-edge size of a thumbnail in pixels.
pub const MAX_THUMBNAIL_PX: u32 = 512;
/// Default longest-edge size of a thumbnail in pixels.
pub const DEFAULT_THUMBNAIL_PX: u32 = 256;

fn default_max_px() -> u32 {
    DEFAULT_THUMBNAIL_PX
}

/// Parameters for `pdf.thumbnails`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThumbnailsParams {
    /// Pages to render (1-based selector).
    #[serde(default)]
    pub pages: PageRange,
    /// Longest edge of each thumbnail in pixels.
    #[serde(default = "default_max_px")]
    #[schemars(range(min = 64, max = 512))]
    pub max_px: u32,
}

impl Default for ThumbnailsParams {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            max_px: DEFAULT_THUMBNAIL_PX,
        }
    }
}

impl ThumbnailsParams {
    /// Checks the range the JSON schema alone cannot enforce.
    pub fn validate(&self) -> Result<(), ParamsError> {
        check_range("max_px", self.max_px, MIN_THUMBNAIL_PX, MAX_THUMBNAIL_PX)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_follow_the_tool_table() {
        let params: ThumbnailsParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params, ThumbnailsParams::default());
        assert_eq!(params.max_px, 256);
        params.validate().expect("defaults validate");
    }

    #[test]
    fn accepts_a_valid_example() {
        let params: ThumbnailsParams =
            serde_json::from_value(json!({"pages": "2-4", "max_px": 512})).expect("valid");
        params.validate().expect("valid");
    }

    #[test]
    fn rejects_invalid_examples() {
        for value in [
            json!({"max_px": "big"}),
            json!({"dpi": 100}),
            json!({"pages": "x"}),
        ] {
            assert!(serde_json::from_value::<ThumbnailsParams>(value).is_err());
        }
        for max_px in [63, 513] {
            let params = ThumbnailsParams {
                max_px,
                ..ThumbnailsParams::default()
            };
            assert!(params.validate().is_err(), "{max_px}");
        }
    }
}
