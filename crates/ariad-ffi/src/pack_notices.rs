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
///
/// When the notices of a pack could not be read, `error` carries the reason and `notices` is
/// empty, so one damaged pack does not hide the others.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FfiPackNotices {
    pub name: String,
    pub version: String,
    pub active: bool,
    pub notices: Vec<FfiPackNotice>,
    pub error: Option<String>,
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
            error: None,
        }
    }
}

impl From<packs::PackNoticesEntry> for FfiPackNotices {
    fn from(entry: packs::PackNoticesEntry) -> Self {
        match entry.result {
            Ok(found) => found.into(),
            Err(error) => Self {
                name: entry.name,
                version: entry.version,
                active: true,
                notices: Vec::new(),
                error: Some(format!("{}: {error}", error.code())),
            },
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
/// Without `name`, returns the active version of every installed pack and ignores `version`;
/// a pack whose notices cannot be read comes back with `error` set instead of failing the call.
/// With `name`, a notice that is missing, altered or not a regular file is an error.
#[uniffi::export]
pub fn pack_notices(
    name: Option<String>,
    version: Option<String>,
) -> Result<Vec<FfiPackNotices>, FfiError> {
    catch_panics(|| {
        let context = packs::PackContext::from_env().map_err(pack_error)?;
        match name {
            Some(name) => {
                let found =
                    packs::notices(&context, &name, version.as_deref()).map_err(pack_error)?;
                Ok(vec![found.into()])
            }
            None => Ok(packs::active_notices(&context)
                .map_err(pack_error)?
                .into_iter()
                .map(FfiPackNotices::from)
                .collect()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, result: Result<packs::PackNotices, PackError>) -> packs::PackNoticesEntry {
        packs::PackNoticesEntry {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            result,
        }
    }

    #[test]
    fn a_failed_pack_is_reported_beside_the_ones_that_worked() {
        let good = packs::PackNotices {
            name: "beta".to_owned(),
            version: "1.0.0".to_owned(),
            active: true,
            notices: vec![packs::PackNotice {
                path: "LICENSE.txt".to_owned(),
                text: "MIT\n".to_owned(),
            }],
        };
        let converted: Vec<FfiPackNotices> = vec![
            entry("alpha", Err(PackError::Usage("broken".to_owned()))),
            entry("beta", Ok(good)),
        ]
        .into_iter()
        .map(FfiPackNotices::from)
        .collect();

        assert_eq!(converted[0].name, "alpha");
        assert!(converted[0].notices.is_empty());
        assert!(
            converted[0]
                .error
                .as_deref()
                .is_some_and(|e| e.contains("broken"))
        );
        assert_eq!(converted[1].error, None);
        assert_eq!(converted[1].notices[0].text, "MIT\n");
    }
}
