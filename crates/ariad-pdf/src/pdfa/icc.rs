//! OutputIntent creation and sRGB ICC profile embedding for PDF/A compliance.

use lopdf::{Dictionary, Document, Object, Stream};

/// Embedded CC0 sRGB-v4 ICC profile from saucecontrol/Compact-ICC-Profiles.
pub const SRGB_V4_ICC: &[u8] = include_bytes!("../../assets/sRGB-v4.icc");

/// Ensures the document has a valid GTS_PDFA1 OutputIntent embedding sRGB-v4.
pub fn ensure_output_intent(doc: &mut Document) -> Result<(), crate::error::PdfError> {
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| crate::error::PdfError::Malformed)?;

    // Check if an existing GTS_PDFA1 OutputIntent is already present with a profile
    let has_valid_intent = {
        let catalog = doc
            .get_object(catalog_id)
            .and_then(Object::as_dict)
            .map_err(|_| crate::error::PdfError::Malformed)?;

        if let Ok(intents) = catalog.get(b"OutputIntents").and_then(Object::as_array) {
            intents.iter().any(|intent_obj| {
                let dict = match intent_obj {
                    Object::Dictionary(d) => Some(d),
                    Object::Reference(r) => doc.get_object(*r).ok().and_then(|o| o.as_dict().ok()),
                    _ => None,
                };
                if let Some(d) = dict {
                    let is_pdfa = d.get(b"S").and_then(Object::as_name).ok() == Some(b"GTS_PDFA1");
                    let has_dest = d.has(b"DestOutputProfile");
                    is_pdfa && has_dest
                } else {
                    false
                }
            })
        } else {
            false
        }
    };

    if has_valid_intent {
        return Ok(());
    }

    // Embed the sRGB-v4 ICC stream
    let mut icc_dict = Dictionary::new();
    icc_dict.set(b"N", Object::Integer(3)); // RGB has 3 color components
    let icc_stream = Stream::new(icc_dict, SRGB_V4_ICC.to_vec());
    let icc_stream_id = doc.add_object(icc_stream);

    // Build the OutputIntent dictionary
    let mut intent_dict = Dictionary::new();
    intent_dict.set(b"Type", Object::Name(b"OutputIntent".to_vec()));
    intent_dict.set(b"S", Object::Name(b"GTS_PDFA1".to_vec()));
    intent_dict.set(
        b"OutputConditionIdentifier",
        Object::string_literal(b"sRGB".to_vec()),
    );
    intent_dict.set(
        b"RegistryName",
        Object::string_literal(b"http://www.color.org".to_vec()),
    );
    intent_dict.set(
        b"Info",
        Object::string_literal(b"sRGB IEC61966-2.1".to_vec()),
    );
    intent_dict.set(b"DestOutputProfile", Object::Reference(icc_stream_id));
    let intent_id = doc.add_object(intent_dict);

    // Add or append to Catalog's OutputIntents
    let catalog = doc
        .get_object_mut(catalog_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| crate::error::PdfError::Malformed)?;

    if let Ok(intents) = catalog
        .get_mut(b"OutputIntents")
        .and_then(Object::as_array_mut)
    {
        intents.push(Object::Reference(intent_id));
    } else {
        catalog.set(
            b"OutputIntents",
            Object::Array(vec![Object::Reference(intent_id)]),
        );
    }

    Ok(())
}
