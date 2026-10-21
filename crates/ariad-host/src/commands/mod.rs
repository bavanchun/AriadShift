use crate::convert::ConvertError;
use ariad_core::planner::{self, Capabilities};

pub mod doctor;
pub mod engines;
pub mod inspect;
pub mod plan;

/// Returns the effective capabilities registry.
/// In test-probe builds, checks `ASHIFT_TEST_CAPABILITIES` first for integration tests.
/// In production/release builds, always uses embedded capabilities.
pub fn active_capabilities() -> Result<Capabilities, ConvertError> {
    #[cfg(feature = "test-probe")]
    if let Some(path) = std::env::var_os("ASHIFT_TEST_CAPABILITIES") {
        let content = std::fs::read_to_string(&path).map_err(|e| {
            ConvertError::InvalidCapabilities(format!("could not read capabilities file: {e}"))
        })?;
        let caps = serde_json::from_str::<Capabilities>(&content).map_err(|e| {
            ConvertError::InvalidCapabilities(format!("malformed capabilities JSON: {e}"))
        })?;
        return Ok(caps);
    }
    Ok(planner::embedded().clone())
}
