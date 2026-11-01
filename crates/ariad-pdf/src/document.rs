//! Bounded PDF document loader and wrapper (`PdfDoc`).
//!
//! All opens delegate to [`ariad_core::reader::pdf::load_bounded`] and enforce encryption
//! provenance so structural tools never silently strip owner-password encryption by saving a
//! decrypted [`lopdf::Document`].

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use ariad_core::{
    limits::Limits,
    reader::pdf::{CONTENT_BUDGET_BASE, CONTENT_BUDGET_PER_INPUT_BYTE, ReadError, load_bounded},
};
use lopdf::{DecompressError, Document, Object, ObjectId, Stream};

use crate::error::PdfError;

/// Maximum byte window scanned after each `trailer` keyword when pre-scanning raw PDF bytes for
/// `/Encrypt` and `/Prev` before `lopdf` parses the document.
const MAX_TRAILER_WINDOW: usize = 4096;

/// Cooperative cancellation and deadline context for in-process PDF tool execution.
#[derive(Clone, Debug, Default)]
pub struct ToolCtx {
    /// Optional wall-clock deadline after which page and object loops abort with
    /// [`PdfError::TIMED_OUT`].
    pub deadline: Option<Instant>,
    /// Shared cancellation flag checked cooperatively by page and object loops.
    pub cancel: Arc<AtomicBool>,
}

impl ToolCtx {
    /// Creates a default context with no deadline and an uncancelled flag.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a context with the given wall-clock `deadline`.
    #[must_use]
    pub fn with_deadline(deadline: Instant) -> Self {
        Self {
            deadline: Some(deadline),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Creates a context sharing the given `cancel` flag.
    #[must_use]
    pub const fn with_cancel(cancel: Arc<AtomicBool>) -> Self {
        Self {
            deadline: None,
            cancel,
        }
    }

    /// Checks whether the cancellation flag has been set or the deadline has elapsed.
    pub fn check(&self) -> Result<(), PdfError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(PdfError::CANCELLED);
        }
        if let Some(deadline) = self.deadline
            && Instant::now() >= deadline
        {
            return Err(PdfError::TIMED_OUT);
        }
        Ok(())
    }
}

/// Per-document decoded-bytes budget derived from [`Limits`].
///
/// Shared across `ObjStm` / `XRef` streams on load, page `/Contents` streams, `/Form` XObject
/// streams, and XMP `/Metadata` streams. Every stream reference charges its decoded byte count
/// against the budget.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodeBudget {
    decoded: u64,
    limit: u64,
}

impl DecodeBudget {
    pub(crate) fn from_limits(input_len: usize, limits: &Limits) -> Self {
        let amplification = CONTENT_BUDGET_BASE
            .saturating_add(CONTENT_BUDGET_PER_INPUT_BYTE.saturating_mul(input_len as u64))
            .min((usize::MAX / 4) as u64);
        let limit = limits.max_decompressed_bytes.min(amplification);
        Self { decoded: 0, limit }
    }

    const fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.decoded)
    }

    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), PdfError> {
        self.decoded = self.decoded.saturating_add(bytes as u64);
        if self.decoded > self.limit {
            return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
                limit: self.limit,
            }));
        }
        Ok(())
    }

    /// Decodes `stream` within the remaining per-document budget using
    /// [`Stream::decompressed_content_with_limit`].
    ///
    /// Returns `Ok(None)` when the stream uses an unsupported or corrupt filter, or
    /// `Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge { .. }))` when decoding
    /// exceeds the per-document budget.
    pub(crate) fn decode(&mut self, stream: &Stream) -> Result<Option<Vec<u8>>, PdfError> {
        let remaining = self.remaining().min((usize::MAX / 4) as u64) as usize;
        match stream.decompressed_content_with_limit(remaining) {
            Ok(data) => {
                self.charge(data.len())?;
                Ok(Some(data))
            }
            Err(lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
                Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
                    limit: self.limit,
                }))
            }
            Err(_) => Ok(None),
        }
    }
}

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
    /// Whether strict parsing failed on the cross-reference table and `lopdf` had to reconstruct
    /// object offsets in non-strict mode.
    pub repaired_xref: bool,
}

/// Bounded wrapper around [`lopdf::Document`] carrying trailer and encryption provenance.
#[derive(Clone, Debug)]
pub struct PdfDoc {
    inner: Document,
    provenance: TrailerProvenance,
    decode_budget: DecodeBudget,
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

    /// Whether strict parsing failed on the cross-reference table and object offsets had to be
    /// reconstructed when opening the document.
    #[must_use]
    pub const fn repaired_xref(&self) -> bool {
        self.provenance.repaired_xref
    }

    /// Returns the per-document stream decompression budget seeded when the document was opened.
    #[must_use]
    pub(crate) const fn decode_budget(&self) -> DecodeBudget {
        self.decode_budget
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
    open_with_ctx(bytes, limits, &ToolCtx::default())
}

/// Opens a PDF document for structural manipulation under `limits` and cooperative `ctx`.
pub fn open_with_ctx(bytes: &[u8], limits: &Limits, ctx: &ToolCtx) -> Result<PdfDoc, PdfError> {
    let doc = open_for_info_with_ctx(bytes, limits, ctx)?;
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
    open_for_info_with_ctx(bytes, limits, &ToolCtx::default())
}

/// Opens a PDF document in read-only inspection mode under `limits` and cooperative `ctx`.
pub fn open_for_info_with_ctx(
    bytes: &[u8],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<PdfDoc, PdfError> {
    ctx.check()?;
    limits.validate().map_err(ReadError::from)?;
    if let Some(limit) = limits.max_input_bytes
        && bytes.len() as u64 > limit
    {
        return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
            actual: bytes.len() as u64,
            limit,
        }));
    }

    let raw_scan = scan_raw_trailers(bytes, ctx)?;
    ctx.check()?;

    let loaded = match load_bounded(bytes, limits) {
        Ok(loaded) => loaded,
        Err(ReadError::Encrypted) => {
            return Err(PdfError::Encrypted {
                needs_password: true,
            });
        }
        Err(other) => return Err(PdfError::from(other)),
    };
    let repaired_xref = loaded.repaired_xref;
    let inner = loaded.document;

    let mut decode_budget = DecodeBudget::from_limits(bytes.len(), limits);
    for object in inner.objects.values() {
        ctx.check()?;
        if let Object::Stream(stream) = object
            && (stream.dict.has_type(b"ObjStm") || stream.dict.has_type(b"XRef"))
        {
            decode_budget.decode(stream)?;
        }
    }

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

    let encrypted = raw_scan.has_encrypt
        || inner.encryption_state.is_some()
        || inner.trailer.get(b"Encrypt").is_ok();

    Ok(PdfDoc {
        inner,
        provenance: TrailerProvenance {
            encrypted,
            encrypt_object_id,
            has_prev_xref: raw_scan.has_prev,
            repaired_xref,
        },
        decode_budget,
    })
}

#[derive(Clone, Copy, Debug, Default)]
struct RawTrailerScan {
    has_encrypt: bool,
    has_prev: bool,
}

/// Scans `bytes` in a single linear pass for `/Encrypt` and `/Prev` keys inside `trailer`
/// sections before `lopdf`'s empty-password decryption removes `/Encrypt` from `doc.trailer`.
///
/// Each `trailer` keyword opens or extends a scan window capped at [`MAX_TRAILER_WINDOW`] bytes
/// (or until `startxref`, whichever comes first). The cursor `i` advances monotonically, so every
/// input byte is visited at most once regardless of how many `trailer` tokens appear in the file.
fn scan_raw_trailers(bytes: &[u8], ctx: &ToolCtx) -> Result<RawTrailerScan, PdfError> {
    const TRAILER: &[u8] = b"trailer";
    const STARTXREF: &[u8] = b"startxref";

    let mut result = RawTrailerScan::default();
    let mut i = 0_usize;
    let mut window_end = 0_usize;
    let mut next_check = 65_536_usize;

    while i < bytes.len() {
        if i >= next_check {
            ctx.check()?;
            next_check = i.saturating_add(65_536);
        }

        if i >= window_end {
            if bytes.len() - i < TRAILER.len() {
                break;
            }
            let Some(rel) = bytes[i..].windows(TRAILER.len()).position(|w| w == TRAILER) else {
                break;
            };
            i = i + rel + TRAILER.len();
            window_end = i.saturating_add(MAX_TRAILER_WINDOW).min(bytes.len());
            ctx.check()?;
            next_check = i.saturating_add(65_536);
            continue;
        }

        let rest = &bytes[i..];
        if rest.starts_with(STARTXREF) {
            i += STARTXREF.len();
            window_end = 0;
            continue;
        }
        if rest.starts_with(TRAILER) {
            i += TRAILER.len();
            window_end = i.saturating_add(MAX_TRAILER_WINDOW).min(bytes.len());
            continue;
        }
        if bytes[i] == b'/' {
            if !result.has_encrypt && matches_pdf_key_at(bytes, i, b"/Encrypt") {
                result.has_encrypt = true;
            } else if !result.has_prev && matches_pdf_key_at(bytes, i, b"/Prev") {
                result.has_prev = true;
            }
            if result.has_encrypt && result.has_prev {
                break;
            }
        }
        i += 1;
    }

    Ok(result)
}

/// Checks whether `bytes[idx..]` starts with the exact PDF name token `key` followed by a PDF
/// delimiter or whitespace byte (so `/Encrypt` does not match `/EncryptMetadata`).
fn matches_pdf_key_at(bytes: &[u8], idx: usize, key: &[u8]) -> bool {
    let Some(rest) = bytes.get(idx..) else {
        return false;
    };
    if !rest.starts_with(key) {
        return false;
    }
    rest.get(key.len())
        .copied()
        .is_none_or(is_pdf_delimiter_or_ws)
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
