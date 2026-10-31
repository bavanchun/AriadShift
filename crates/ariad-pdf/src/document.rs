//! Bounded PDF document loader and wrapper (`PdfDoc`).
//!
//! All opens delegate to [`ariad_core::reader::pdf::load_bounded`] and enforce encryption
//! provenance so structural tools never silently strip owner-password encryption by saving a
//! decrypted [`lopdf::Document`].

use ariad_core::{
    limits::Limits,
    reader::pdf::{ReadError, load_bounded},
};
use lopdf::{Document, ObjectId};

use crate::error::PdfError;

/// Provenance recorded from the PDF trailer and encryption state at load time.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrailerProvenance {
    /// Whether the input trailer declared an `/Encrypt` dictionary (even if `lopdf`
    /// transparently decrypted the document with the empty user password).
    pub encrypted: bool,
    /// Object ID of the original `/Encrypt` dictionary in the trailer, when indirect.
    pub encrypt_object_id: Option<ObjectId>,
    /// Whether the document had `/Prev` incremental cross-reference sections in the raw bytes.
    pub has_prev_xref: bool,
}

/// Bounded wrapper around [`lopdf::Document`] carrying trailer and encryption provenance.
#[derive(Clone, Debug)]
pub struct PdfDoc {
    inner: Document,
    provenance: TrailerProvenance,
}

impl PdfDoc {
    /// Borrows the underlying [`lopdf::Document`].
    #[must_use]
    pub const fn inner(&self) -> &Document {
        &self.inner
    }

    /// Mutably borrows the underlying [`lopdf::Document`].
    ///
    /// Returns [`PdfError::Encrypted`] if this handle was opened from an encrypted document via
    /// the read-only inspection path.
    pub fn inner_mut(&mut self) -> Result<&mut Document, PdfError> {
        if self.provenance.encrypted {
            return Err(PdfError::Encrypted {
                needs_password: false,
            });
        }
        Ok(&mut self.inner)
    }

    /// Consumes the wrapper and returns the underlying [`lopdf::Document`].
    ///
    /// Returns [`PdfError::Encrypted`] if this handle was opened from an encrypted document.
    pub fn into_inner(self) -> Result<Document, PdfError> {
        if self.provenance.encrypted {
            return Err(PdfError::Encrypted {
                needs_password: false,
            });
        }
        Ok(self.inner)
    }

    /// Returns the trailer and encryption provenance recorded when the document was opened.
    #[must_use]
    pub const fn provenance(&self) -> &TrailerProvenance {
        &self.provenance
    }

    /// Whether the source PDF was encrypted (including owner-password-only encryption).
    #[must_use]
    pub const fn is_encrypted(&self) -> bool {
        self.provenance.encrypted
    }

    /// Serializes the document to PDF bytes.
    ///
    /// Refuses to serialize any document that was opened from an encrypted source without an
    /// explicit unlock operation, preventing structural tools from silently stripping
    /// owner-password protection.
    pub fn save_bytes(&mut self) -> Result<Vec<u8>, PdfError> {
        if self.provenance.encrypted {
            return Err(PdfError::Encrypted {
                needs_password: false,
            });
        }
        let mut out = Vec::new();
        self.inner
            .save_to(&mut out)
            .map_err(|_| PdfError::Malformed)?;
        Ok(out)
    }
}

/// Opens a PDF document for structural manipulation under `limits`.
///
/// Delegates to [`ariad_core::reader::pdf::load_bounded`] and inspects the raw trailer and
/// [`lopdf::Document::encryption_state`] for `/Encrypt`:
/// - If the document requires a non-empty user password, returns
///   `Err(PdfError::Encrypted { needs_password: true })`.
/// - If the document was encrypted with an empty user password (owner-password-only protection),
///   returns `Err(PdfError::Encrypted { needs_password: false })` so structural tools never save
///   an unencrypted copy without explicit unlock.
pub fn open(bytes: &[u8], limits: &Limits) -> Result<PdfDoc, PdfError> {
    let doc = open_for_info(bytes, limits)?;
    if doc.is_encrypted() {
        return Err(PdfError::Encrypted {
            needs_password: false,
        });
    }
    Ok(doc)
}

/// Opens a PDF document in read-only inspection mode (used by `pdf.info`).
///
/// Owner-password-only documents (which decrypt with the empty user password) succeed with
/// `doc.is_encrypted() == true`, while their [`PdfDoc::save_bytes`] and [`PdfDoc::inner_mut`]
/// methods remain locked against mutation or decrypted saving. Documents requiring a non-empty
/// user password still fail with `Err(PdfError::Encrypted { needs_password: true })`.
pub fn open_for_info(bytes: &[u8], limits: &Limits) -> Result<PdfDoc, PdfError> {
    let raw_encrypted = raw_trailer_has_encrypt(bytes);
    let has_prev_xref = raw_trailer_has_prev(bytes);

    let inner = match load_bounded(bytes, limits) {
        Ok(doc) => doc,
        Err(ReadError::Encrypted) => {
            return Err(PdfError::Encrypted {
                needs_password: true,
            });
        }
        Err(other) => return Err(PdfError::from(other)),
    };

    let encrypt_object_id = inner
        .encryption_state
        .as_ref()
        .and_then(lopdf::EncryptionState::encrypt_object_id)
        .or_else(|| {
            inner
                .trailer
                .get(b"Encrypt")
                .ok()
                .and_then(|obj| obj.as_reference().ok())
        });

    let encrypted =
        raw_encrypted || inner.encryption_state.is_some() || inner.trailer.get(b"Encrypt").is_ok();

    Ok(PdfDoc {
        inner,
        provenance: TrailerProvenance {
            encrypted,
            encrypt_object_id,
            has_prev_xref,
        },
    })
}

/// Scans raw trailer sections in `bytes` for a `/Encrypt` key before `lopdf`'s empty-password
/// decryption removes `/Encrypt` from `doc.trailer`.
fn raw_trailer_has_encrypt(bytes: &[u8]) -> bool {
    for section in trailer_slices(bytes) {
        if contains_pdf_key(section, b"/Encrypt") {
            return true;
        }
    }
    false
}

/// Scans raw trailer sections in `bytes` for a `/Prev` cross-reference pointer.
fn raw_trailer_has_prev(bytes: &[u8]) -> bool {
    for section in trailer_slices(bytes) {
        if contains_pdf_key(section, b"/Prev") {
            return true;
        }
    }
    false
}

/// Yields byte slices between `trailer` and `startxref` in `bytes`.
fn trailer_slices(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    const TRAILER: &[u8] = b"trailer";
    const STARTXREF: &[u8] = b"startxref";

    let mut pos = 0;
    std::iter::from_fn(move || {
        if pos + TRAILER.len() > bytes.len() {
            return None;
        }
        let rel = bytes[pos..]
            .windows(TRAILER.len())
            .position(|w| w == TRAILER)?;
        let start = pos + rel + TRAILER.len();
        pos = start;
        let len = bytes[start..]
            .windows(STARTXREF.len())
            .position(|w| w == STARTXREF)
            .unwrap_or_else(|| (bytes.len() - start).min(4096));
        Some(&bytes[start..start + len])
    })
}

/// Checks whether `haystack` contains the exact PDF name token `key` (followed by a PDF
/// delimiter or whitespace byte, so `/Encrypt` does not match `/EncryptMetadata`).
fn contains_pdf_key(haystack: &[u8], key: &[u8]) -> bool {
    if haystack.len() < key.len() {
        return false;
    }
    for (idx, window) in haystack.windows(key.len()).enumerate() {
        if window == key {
            let next = haystack.get(idx + key.len()).copied();
            if next.is_none_or(is_pdf_delimiter_or_ws) {
                return true;
            }
        }
    }
    false
}

const fn is_pdf_delimiter_or_ws(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t'
            | b'\r'
            | b'\n'
            | b'\x0c'
            | b'\0'
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'/'
            | b'%'
    )
}
