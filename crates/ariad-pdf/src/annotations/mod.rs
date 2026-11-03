//! Bounded PDF annotation operations.

#[cfg(feature = "codecs")]
pub mod appearance;
#[cfg(feature = "codecs")]
pub mod flatten;
#[cfg(feature = "codecs")]
pub mod modify;
pub mod read;
#[cfg(feature = "codecs")]
pub mod run;
#[cfg(feature = "codecs")]
pub mod write;

use ariad_core::tools::params::annotations::{AnnotationInfo, AnnotationType};
use serde::Serialize;

/// Pages and optional annotation filters.
#[derive(Clone, Debug)]
pub struct AnnotationSelector {
    pub pages: Vec<u32>,
    pub ids: Option<Vec<String>>,
    pub types: Option<Vec<AnnotationType>>,
    pub all: bool,
}

/// Static annotation metadata and any non-fatal diagnostics.
#[derive(Clone, Debug, Default, Serialize)]
pub struct AnnotationListing {
    pub annotations: Vec<AnnotationInfo>,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

/// Summary of a successful annotation edit.
#[derive(Clone, Debug, Default)]
pub struct EditReport {
    pub affected: Vec<String>,
    pub warnings: Vec<String>,
}
