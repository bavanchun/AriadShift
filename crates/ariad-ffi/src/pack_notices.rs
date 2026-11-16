//! Licence notices of installed engine packs, for the app's licence screen.

use ariad_host::packs::{self, PackError};

use crate::{FfiError, catch_panics};

/// One notice file shipped inside an installed pack.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FfiPackNotice {
    /// Path inside the pack, as listed by the signed manifest.
    pub path: String,
    pub text: String,
}

/// The notices of one installed pack version.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FfiPackNotices {
    pub name: String,
    pub version: String,
    pub active: bool,
    pub notices: Vec<FfiPackNotice>,
}

impl From<packs::PackNotices> for FfiPackNotices {
    fn from(found: packs::PackNotices) -> Self {
        Self {
            name: found.name,
            version: found.version,
            active: found.active,
            notices: found
                .notices
                .into_iter()
                .map(|notice| FfiPackNotice {
                    path: notice.path,
                    text: notice.text,
                })
                .collect(),
        }
    }
}

fn pack_error(error: PackError) -> FfiError {
    match error {
        PackError::Usage(message) => FfiError::InvalidRequest { message },
        PackError::Interrupted => FfiError::Cancelled,
        other => FfiError::Internal {
            message: format!("{}: {other}", other.code()),
        },
    }
}

/// Reads the licence notices of installed engine packs.
///
/// With `name`, returns that pack's notices for `version` (the active version when `None`).
/// Without `name`, returns the active version of every installed pack and ignores `version`.
/// Fails when a listed notice file is missing or is not a regular file.
#[uniffi::export]
pub fn pack_notices(
    name: Option<String>,
    version: Option<String>,
) -> Result<Vec<FfiPackNotices>, FfiError> {
    catch_panics(|| {
        let context = packs::PackContext::from_env().map_err(pack_error)?;
        let found = match name {
            Some(name) => {
                vec![packs::notices(&context, &name, version.as_deref()).map_err(pack_error)?]
            }
            None => packs::active_notices(&context).map_err(pack_error)?,
        };
        Ok(found.into_iter().map(FfiPackNotices::from).collect())
    })
}
