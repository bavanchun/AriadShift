//! Conservative PDF/A fixer (`pdf.to_pdfa`).

use std::collections::BTreeSet;

use lopdf::{Dictionary, Document, Object, ObjectId};

use super::{fonts, icc, xmp};
use crate::error::PdfError;

/// Conformance level for PDF/A conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixerLevel {
    A2b,
    A3b,
    A2u,
}

impl FixerLevel {
    pub fn part(&self) -> u32 {
        match self {
            Self::A2b | Self::A2u => 2,
            Self::A3b => 3,
        }
    }

    pub fn conformance(&self) -> &'static str {
        match self {
            Self::A2b | Self::A3b => "B",
            Self::A2u => "U",
        }
    }

    pub fn allows_embedded_files(&self) -> bool {
        match self {
            Self::A2b | Self::A2u => false,
            Self::A3b => true,
        }
    }

    pub fn requires_to_unicode(&self) -> bool {
        match self {
            Self::A2u => true,
            Self::A2b | Self::A3b => false,
        }
    }
}

/// Runs the conservative PDF/A fixer on an open `Document`.
pub fn fix_pdfa_document(
    doc: &mut Document,
    level: FixerLevel,
    timestamp: i64,
) -> Result<(), PdfError> {
    // 1. Refuse encrypted input
    if doc.is_encrypted() || doc.trailer.has(b"Encrypt") {
        return Err(PdfError::Encrypted {
            needs_password: true,
        });
    }

    // 2. Refuse embedded files for 2b and 2u
    if !level.allows_embedded_files() && has_embedded_files(doc)? {
        return Err(PdfError::InvalidParams(
            "PDF/A-2b and PDF/A-2u forbid embedded files; use level 3b or remove attachments"
                .into(),
        ));
    }

    // 3. Strip JavaScript and forbidden launch/sound/movie actions
    strip_active_content(doc)?;

    // 4. Ensure OutputIntent with sRGB-v4 profile
    icc::ensure_output_intent(doc)?;

    // 5. Ensure XMP metadata with pdfaid part and conformance
    xmp::ensure_pdfa_metadata(doc, level.part(), level.conformance(), timestamp)?;

    // 6. Ensure fonts are embedded (Standard-14 via Liberation 2.1.5)
    fonts::ensure_fonts_embedded(doc, level.requires_to_unicode())?;

    Ok(())
}

fn has_embedded_files(doc: &Document) -> Result<bool, PdfError> {
    fn contains(object: &Object) -> bool {
        match object {
            Object::Dictionary(dict) => {
                dict.has(b"EmbeddedFiles")
                    || dict.has(b"EF")
                    || dict.get(b"Subtype").and_then(Object::as_name).ok()
                        == Some(b"FileAttachment")
                    || dict.iter().any(|(_, value)| contains(value))
            }
            Object::Stream(stream) => {
                stream.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"EmbeddedFile")
                    || stream.dict.iter().any(|(_, value)| contains(value))
            }
            Object::Array(array) => array.iter().any(contains),
            _ => false,
        }
    }
    Ok(doc.objects.values().any(contains))
}

fn strip_active_content(doc: &mut Document) -> Result<(), PdfError> {
    let forbidden: BTreeSet<ObjectId> = doc
        .objects
        .iter()
        .filter_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|dict| is_action_forbidden(dict))
                .map(|_| *id)
        })
        .collect();
    for object in doc.objects.values_mut() {
        clean_object(object, &forbidden);
    }
    Ok(())
}

fn clean_object(object: &mut Object, forbidden: &BTreeSet<ObjectId>) {
    match object {
        Object::Reference(id) if forbidden.contains(id) => *object = Object::Null,
        Object::Dictionary(dict) if is_action_forbidden(dict) => *object = Object::Null,
        Object::Dictionary(dict) => clean_dict(dict, forbidden),
        Object::Stream(stream) => clean_dict(&mut stream.dict, forbidden),
        Object::Array(array) => {
            for value in array.iter_mut() {
                clean_object(value, forbidden);
            }
            array.retain(|value| !matches!(value, Object::Null));
        }
        _ => {}
    }
}

fn clean_dict(dict: &mut Dictionary, forbidden: &BTreeSet<ObjectId>) {
    for key in [b"JS".as_slice(), b"JavaScript", b"AA"] {
        dict.remove(key);
    }
    for (_, value) in dict.iter_mut() {
        clean_object(value, forbidden);
    }
    // A removed action must not leave an invalid null action entry behind.
    for key in [b"A".as_slice(), b"OpenAction", b"Next"] {
        if matches!(dict.get(key), Ok(Object::Null)) {
            dict.remove(key);
        }
    }
}

fn is_action_forbidden(dict: &Dictionary) -> bool {
    if let Ok(subtype) = dict.get(b"S").and_then(Object::as_name) {
        matches!(
            subtype,
            b"JavaScript"
                | b"Launch"
                | b"Sound"
                | b"Movie"
                | b"ResetForm"
                | b"ImportData"
                | b"Hide"
                | b"SetState"
                | b"NOP"
        )
    } else {
        false
    }
}
