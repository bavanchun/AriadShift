//! XMP metadata update and timestamp/ID synchronization for PDF/A compliance.

use lopdf::{Dictionary, Document, Object, Stream};

/// Ensures the document has a valid PDF/A XMP metadata stream, matching `/Info` timestamps,
/// and a trailer `/ID` array.
pub fn ensure_pdfa_metadata(
    doc: &mut Document,
    part: u32,
    conformance: &str,
    timestamp_secs: i64,
) -> Result<(), crate::error::PdfError> {
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| crate::error::PdfError::Malformed)?;

    // Format ISO 8601 string and PDF date string from timestamp
    let (iso_date, pdf_date) = format_dates(timestamp_secs);

    // Ensure trailer /ID exists
    ensure_trailer_id(doc, timestamp_secs);

    // Sync /Info dictionary dates
    sync_info_dates(doc, &pdf_date);

    // Build or update the XMP metadata stream
    let existing_meta_id = doc
        .get_object(catalog_id)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|c| c.get(b"Metadata").ok())
        .and_then(|o| o.as_reference().ok());

    let description = description(doc, part, conformance, &iso_date)?;
    let updated_xmp = if let Some(meta_id) = existing_meta_id {
        if let Ok(stream) = doc.get_object(meta_id).and_then(Object::as_stream) {
            let decompressed = metadata_bytes(stream)?;
            let existing_xml = std::str::from_utf8(&decompressed).map_err(|_| {
                crate::error::PdfError::InvalidParams("existing XMP is not UTF-8".into())
            })?;
            patch_xmp(existing_xml, &description)?
        } else {
            generate_new_xmp(&description)
        }
    } else {
        generate_new_xmp(&description)
    };

    let mut meta_dict = Dictionary::new();
    meta_dict.set(b"Type", Object::Name(b"Metadata".to_vec()));
    meta_dict.set(b"Subtype", Object::Name(b"XML".to_vec()));
    let meta_stream = Stream::new(meta_dict, updated_xmp.into_bytes());

    if let Some(meta_id) = existing_meta_id {
        doc.set_object(meta_id, meta_stream);
    } else {
        let meta_id = doc.add_object(meta_stream);
        let catalog = doc
            .get_object_mut(catalog_id)
            .and_then(Object::as_dict_mut)
            .map_err(|_| crate::error::PdfError::Malformed)?;
        catalog.set(b"Metadata", Object::Reference(meta_id));
    }

    Ok(())
}

fn format_dates(epoch_secs: i64) -> (String, String) {
    let clamped = epoch_secs.clamp(0, 253_402_300_799);
    let days = clamped.div_euclid(86_400);
    let secs_of_day = clamped.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Howard Hinnant's civil_from_days algorithm
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    let iso = format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}+00:00");
    let pdf = format!("D:{year:04}{m:02}{d:02}{hour:02}{minute:02}{second:02}Z");
    (iso, pdf)
}

fn ensure_trailer_id(doc: &mut Document, timestamp_secs: i64) {
    if !doc.trailer.has(b"ID") {
        let hash = format!("{:016x}{:016x}", timestamp_secs, doc.objects.len());
        let id1 = Object::string_literal(hash.as_bytes().to_vec());
        let id2 = Object::string_literal(hash.as_bytes().to_vec());
        doc.trailer.set(b"ID", Object::Array(vec![id1, id2]));
    }
}

fn sync_info_dates(doc: &mut Document, pdf_date: &str) {
    let info_id = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| o.as_reference().ok());
    if let Some(id) = info_id {
        if let Ok(info) = doc.get_object_mut(id).and_then(Object::as_dict_mut) {
            info.set(
                b"ModDate",
                Object::string_literal(pdf_date.as_bytes().to_vec()),
            );
            info.set(
                b"CreationDate",
                Object::string_literal(pdf_date.as_bytes().to_vec()),
            );
        }
    } else {
        let mut info = Dictionary::new();
        info.set(
            b"ModDate",
            Object::string_literal(pdf_date.as_bytes().to_vec()),
        );
        info.set(
            b"CreationDate",
            Object::string_literal(pdf_date.as_bytes().to_vec()),
        );
        info.set(b"Producer", Object::string_literal(b"AriadShift".to_vec()));
        let id = doc.add_object(info);
        doc.trailer.set(b"Info", Object::Reference(id));
    }
}

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const PDFA_NS: &str = "http://www.aiim.org/pdfa/ns/id/";
const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const PDF_NS: &str = "http://ns.adobe.com/pdf/1.3/";

fn replaced_property(namespace: &str, local: &str) -> bool {
    namespace == PDFA_NS
        || match namespace {
            XMP_NS => matches!(
                local,
                "CreateDate" | "ModifyDate" | "MetadataDate" | "CreatorTool"
            ),
            DC_NS => matches!(local, "title" | "creator" | "description" | "format"),
            PDF_NS => matches!(local, "Producer" | "Keywords"),
            _ => false,
        }
}

// Work with namespace URIs, including attribute properties and self-closing descriptions.
// Preserve unrelated XMP instead of replacing the document's metadata wholesale.
fn patch_xmp(existing: &str, description: &str) -> Result<String, crate::error::PdfError> {
    use quick_xml::{Writer, events::Event, name::ResolveResult, reader::NsReader};
    let malformed = || {
        crate::error::PdfError::InvalidParams(
            "existing XMP cannot be conservatively updated".into(),
        )
    };
    let mut reader = NsReader::from_str(existing);
    let mut writer = Writer::new(Vec::new());
    let mut skipped_depth = 0;
    let mut inserted = false;
    loop {
        let event = reader.read_event().map_err(|_| malformed())?;
        if skipped_depth > 0 {
            match event {
                Event::Start(_) => skipped_depth += 1,
                Event::End(_) => skipped_depth -= 1,
                Event::Eof => return Err(malformed()),
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let (ns, local) = reader.resolver().resolve_element(start.name());
                let namespace = match ns {
                    ResolveResult::Bound(ns) => ns.0,
                    _ => "",
                };
                if replaced_property(namespace, local.as_ref()) {
                    if matches!(event, Event::Start(_)) {
                        skipped_depth = 1;
                    }
                    continue;
                }
                let mut clean = start.clone().into_owned();
                clean.clear_attributes();
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|_| malformed())?;
                    let (ns, local) = reader.resolver().resolve_attribute(attribute.key);
                    if let ResolveResult::Bound(ns) = ns
                        && replaced_property(ns.0, local.as_ref())
                    {
                        continue;
                    }
                    clean.push_attribute(attribute);
                }
                if namespace == RDF_NS
                    && local.as_ref() == "RDF"
                    && matches!(event, Event::Empty(_))
                {
                    writer
                        .write_event(Event::Start(clean.clone()))
                        .map_err(|_| malformed())?;
                    writer.get_mut().extend_from_slice(description.as_bytes());
                    writer
                        .write_event(Event::End(clean.to_end()))
                        .map_err(|_| malformed())?;
                    inserted = true;
                } else {
                    writer
                        .write_event(if matches!(event, Event::Start(_)) {
                            Event::Start(clean)
                        } else {
                            Event::Empty(clean)
                        })
                        .map_err(|_| malformed())?;
                }
            }
            Event::End(ref end) => {
                let (ns, local) = reader.resolver().resolve_element(end.name());
                if matches!(ns, ResolveResult::Bound(ns) if ns.0 == RDF_NS)
                    && local.as_ref() == "RDF"
                {
                    writer.get_mut().extend_from_slice(description.as_bytes());
                    inserted = true;
                }
                writer.write_event(event).map_err(|_| malformed())?;
            }
            Event::Eof => break,
            _ => writer.write_event(event).map_err(|_| malformed())?,
        }
    }
    if !inserted {
        return Err(malformed());
    }
    String::from_utf8(writer.into_inner()).map_err(|_| malformed())
}

fn description(
    doc: &Document,
    part: u32,
    conformance: &str,
    iso_date: &str,
) -> Result<String, crate::error::PdfError> {
    let info = doc
        .trailer
        .get_deref(b"Info", doc)
        .and_then(Object::as_dict)
        .map_err(|_| crate::error::PdfError::Malformed)?;
    let mut properties = String::new();
    for (key, property, container) in [
        (b"Title".as_slice(), "dc:title", "alt"),
        (b"Author", "dc:creator", "seq"),
        (b"Subject", "dc:description", "alt"),
        (b"Keywords", "pdf:Keywords", ""),
        (b"Creator", "xmp:CreatorTool", ""),
        (b"Producer", "pdf:Producer", ""),
    ] {
        let Some(text) = info
            .get(key)
            .ok()
            .and_then(crate::tools::outline::decode_pdf_object_string)
        else {
            continue;
        };
        let text = quick_xml::escape::escape(&text);
        let value = match container {
            "alt" => format!("<rdf:Alt><rdf:li xml:lang=\"x-default\">{text}</rdf:li></rdf:Alt>"),
            "seq" => format!("<rdf:Seq><rdf:li>{text}</rdf:li></rdf:Seq>"),
            _ => text.into_owned(),
        };
        properties.push_str(&format!("<{property}>{value}</{property}>"));
    }
    Ok(format!(
        "<rdf:Description rdf:about=\"\" xmlns:rdf=\"{RDF_NS}\" xmlns:pdfaid=\"{PDFA_NS}\" xmlns:xmp=\"{XMP_NS}\" xmlns:dc=\"{DC_NS}\" xmlns:pdf=\"{PDF_NS}\"><pdfaid:part>{part}</pdfaid:part><pdfaid:conformance>{conformance}</pdfaid:conformance><xmp:CreateDate>{iso_date}</xmp:CreateDate><xmp:ModifyDate>{iso_date}</xmp:ModifyDate><xmp:MetadataDate>{iso_date}</xmp:MetadataDate><dc:format>application/pdf</dc:format>{properties}</rdf:Description>"
    ))
}

fn generate_new_xmp(description: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF_NS}\">{description}</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    )
}

/// Bound metadata decoding even when inspection is called on a raw lopdf document.
pub(super) fn metadata_bytes(stream: &Stream) -> Result<Vec<u8>, crate::error::PdfError> {
    const MAX_XMP_BYTES: usize = 16 * 1024 * 1024;
    if !stream.dict.has(b"Filter") {
        return if stream.content.len() <= MAX_XMP_BYTES {
            Ok(stream.content.clone())
        } else {
            Err(crate::error::PdfError::InvalidParams(
                "XMP metadata exceeds 16 MiB".into(),
            ))
        };
    }
    stream
        .decompressed_content_with_limit(MAX_XMP_BYTES)
        .map_err(|_| {
            crate::error::PdfError::InvalidParams(
                "XMP metadata cannot be decoded within 16 MiB".into(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_replaces_attribute_properties_and_preserves_custom_namespaces() {
        let xml = format!(
            "<rdf:RDF xmlns:rdf=\"{RDF_NS}\"><rdf:Description xmlns:a=\"{PDFA_NS}\" xmlns:xmp=\"{XMP_NS}\" xmlns:custom=\"urn:test\" a:part=\"1\" a:conformance=\"B\" xmp:CreateDate=\"old\" custom:value=\"preserve\"/></rdf:RDF>"
        );
        let output = patch_xmp(&xml, "<new>replacement</new>").unwrap();
        assert!(!output.contains("a:part="));
        assert!(!output.contains("xmp:CreateDate="));
        assert!(output.contains("custom:value=\"preserve\""));
        assert!(output.contains("<new>replacement</new>"));
        assert!(patch_xmp("<broken", "").is_err());
    }
}
