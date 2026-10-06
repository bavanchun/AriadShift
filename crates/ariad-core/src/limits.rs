use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_SUPPORTED_NESTING_DEPTH: u16 = 100;

/// Resource limits applied to one conversion.
///
/// Cloud plan entitlements (§9.5) fill this shape; `None` means unlimited.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Limits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_asset_bytes: Option<u64>,
    pub max_nesting_depth: u16,
    pub max_blocks: u32,
    pub max_front_matter_bytes: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self::local()
    }
}

impl Limits {
    /// Local defaults leave workload-sized limits unlimited.
    #[must_use]
    pub const fn local() -> Self {
        Self {
            max_input_bytes: None,
            max_pages: None,
            timeout_s: None,
            max_memory_mb: None,
            max_asset_bytes: None,
            max_nesting_depth: 64,
            max_blocks: 10_000_000,
            max_front_matter_bytes: 64 * 1024,
        }
    }

    /// Cloud defaults bound workload-sized limits.
    #[must_use]
    pub const fn cloud() -> Self {
        Self {
            max_input_bytes: Some(2 * 1024 * 1024 * 1024),
            max_pages: Some(5_000),
            timeout_s: Some(600),
            max_memory_mb: Some(4_096),
            max_asset_bytes: Some(50 * 1024 * 1024),
            max_nesting_depth: 64,
            max_blocks: 1_000_000,
            max_front_matter_bytes: 64 * 1024,
        }
    }

    /// Checks that finite caps are usable and supported by the readers.
    pub fn validate(&self) -> Result<(), LimitsError> {
        for (field, value) in [
            ("max_input_bytes", self.max_input_bytes),
            ("max_pages", self.max_pages.map(u64::from)),
            ("timeout_s", self.timeout_s),
            ("max_memory_mb", self.max_memory_mb.map(u64::from)),
            ("max_asset_bytes", self.max_asset_bytes),
        ] {
            if value == Some(0) {
                return Err(LimitsError::ZeroLimit { field });
            }
        }
        if self.max_nesting_depth >= MAX_SUPPORTED_NESTING_DEPTH {
            return Err(LimitsError::NestingDepthTooHigh {
                actual: self.max_nesting_depth,
                maximum_exclusive: MAX_SUPPORTED_NESTING_DEPTH,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum LimitsError {
    #[error("{field} must be greater than zero when set")]
    ZeroLimit { field: &'static str },
    #[error("max_nesting_depth must be less than {maximum_exclusive}, got {actual}")]
    NestingDepthTooHigh { actual: u16, maximum_exclusive: u16 },
}

#[cfg(test)]
mod tests {
    use super::{Limits, LimitsError};

    #[test]
    fn local_and_cloud_defaults_match_the_contract() {
        let local = Limits::local();
        assert_eq!(local.max_input_bytes, None);
        assert_eq!(local.max_pages, None);
        assert_eq!(local.timeout_s, None);
        assert_eq!(local.max_memory_mb, None);
        assert_eq!(local.max_asset_bytes, None);
        assert_eq!(local.max_nesting_depth, 64);
        assert_eq!(local.max_blocks, 10_000_000);
        assert_eq!(local.max_front_matter_bytes, 64 * 1024);

        let cloud = Limits::cloud();
        assert_eq!(cloud.max_input_bytes, Some(2 * 1024 * 1024 * 1024));
        assert_eq!(cloud.max_pages, Some(5_000));
        assert_eq!(cloud.timeout_s, Some(600));
        assert_eq!(cloud.max_memory_mb, Some(4_096));
        assert_eq!(cloud.max_asset_bytes, Some(50 * 1024 * 1024));
        assert_eq!(cloud.max_nesting_depth, 64);
        assert_eq!(cloud.max_blocks, 1_000_000);
        assert_eq!(cloud.max_front_matter_bytes, 64 * 1024);
        assert!(local.validate().is_ok());
        assert!(cloud.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_optional_caps() {
        let mut limits = Limits::local();
        limits.max_input_bytes = Some(0);
        assert_eq!(
            limits.validate(),
            Err(LimitsError::ZeroLimit {
                field: "max_input_bytes"
            })
        );

        let mut limits = Limits::local();
        limits.max_pages = Some(0);
        assert!(matches!(
            limits.validate(),
            Err(LimitsError::ZeroLimit { .. })
        ));

        let mut limits = Limits::local();
        limits.timeout_s = Some(0);
        assert!(matches!(
            limits.validate(),
            Err(LimitsError::ZeroLimit { .. })
        ));

        let mut limits = Limits::local();
        limits.max_memory_mb = Some(0);
        assert!(matches!(
            limits.validate(),
            Err(LimitsError::ZeroLimit { .. })
        ));

        let mut limits = Limits::local();
        limits.max_asset_bytes = Some(0);
        assert!(matches!(
            limits.validate(),
            Err(LimitsError::ZeroLimit { .. })
        ));
    }

    #[test]
    fn validate_rejects_nesting_depth_of_one_hundred_or_more() {
        let mut limits = Limits::local();
        limits.max_nesting_depth = 100;
        assert!(matches!(
            limits.validate(),
            Err(LimitsError::NestingDepthTooHigh { actual: 100, .. })
        ));
    }

    #[test]
    fn absent_json_fields_use_local_defaults() {
        let limits: Limits = serde_json::from_str("{}").unwrap();
        assert_eq!(limits, Limits::local());
    }
}
