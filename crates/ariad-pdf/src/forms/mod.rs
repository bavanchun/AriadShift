//! Bounded AcroForm field operations.

#[cfg(feature = "codecs")]
pub mod appearance;
#[cfg(feature = "codecs")]
pub mod create;
#[cfg(feature = "codecs")]
pub mod fill;
#[cfg(feature = "codecs")]
pub mod flatten;
pub mod list;
#[cfg(feature = "codecs")]
pub mod run;

pub(crate) fn resolve<'a>(
    doc: &'a lopdf::Document,
    mut value: &'a lopdf::Object,
    limit: u16,
) -> Result<&'a lopdf::Object, crate::PdfError> {
    let mut seen = std::collections::HashSet::new();
    while let lopdf::Object::Reference(id) = value {
        if seen.len() >= usize::from(limit) || !seen.insert(*id) {
            return Err(crate::PdfError::Malformed);
        }
        value = doc
            .get_object(*id)
            .map_err(|_| crate::PdfError::Malformed)?;
    }
    Ok(value)
}

/// Non-sensitive summary of a form edit.
#[derive(Clone, Debug, Default)]
pub struct EditReport {
    pub affected: Vec<String>,
    pub warnings: Vec<String>,
}
