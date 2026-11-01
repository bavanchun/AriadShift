//! Implementation of the `pdf.metadata` tool.

use std::collections::BTreeMap;

use ariad_core::tools::params::metadata::{MetadataFields, MetadataParams};
use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat, dictionary};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    info, naming,
    tools::OutputDoc,
    writer::SaveOptions,
};

const STANDARD_KEYS: &[&[u8]] = &[
    b"Title",
    b"Author",
    b"Subject",
    b"Keywords",
    b"Creator",
    b"Producer",
    b"CreationDate",
    b"ModDate",
];

#[derive(Clone, Debug, Default)]
struct MetadataState {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
    creator: Option<String>,
    producer: Option<String>,
    created: Option<String>,
    modified: Option<String>,
    language: Option<String>,
    custom: BTreeMap<String, String>,
}

impl MetadataState {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.author.is_none()
            && self.subject.is_none()
            && self.keywords.is_none()
            && self.creator.is_none()
            && self.producer.is_none()
            && self.created.is_none()
            && self.modified.is_none()
            && self.custom.is_empty()
    }
}

/// Updates, removes, or strips document metadata across `/Info` and XMP `/Metadata`.
pub fn run(
    inputs: &[PdfDoc],
    params: &MetadataParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.metadata requires exactly 1 input PDF, got {}",
            inputs.len()
        )));
    }

    let has_set = params.set.as_ref().is_some_and(has_any_metadata_field);
    let has_remove = params.remove.as_ref().is_some_and(|keys| !keys.is_empty());
    if !params.strip_all && !has_set && !has_remove {
        return Err(PdfError::InvalidParams(
            "pdf.metadata requires at least one field in `set`, a non-empty `remove` list, or `strip_all: true`"
                .to_owned(),
        ));
    }
    if let Some(keys) = &params.remove {
        if keys.is_empty() {
            return Err(PdfError::InvalidParams(
                "pdf.metadata `remove` list must not be empty when provided".to_owned(),
            ));
        }
        for key in keys {
            if key.trim().is_empty() {
                return Err(PdfError::InvalidParams(
                    "pdf.metadata `remove` keys must not be empty".to_owned(),
                ));
            }
        }
    }
    if let Some(fields) = &params.set {
        for key in fields.custom.keys() {
            if key.trim().is_empty() {
                return Err(PdfError::InvalidParams(
                    "pdf.metadata `set.custom` keys must not be empty".to_owned(),
                ));
            }
        }
    }

    let mut pdf = inputs[0].clone();
    let stem = naming::doc_stem(&pdf).unwrap_or_else(|| "document".to_owned());

    let mut state = if params.strip_all {
        MetadataState::default()
    } else {
        read_existing_metadata(&pdf, ctx)?
    };

    if let Some(keys) = &params.remove {
        for raw_key in keys {
            apply_remove_key(&mut state, raw_key.trim());
        }
    }

    if let Some(fields) = &params.set {
        apply_set_fields(&mut state, fields);
    }

    let doc = pdf.inner_mut()?;
    let save_options = if state.is_empty() && params.strip_all {
        remove_info_and_xmp(doc)?;
        SaveOptions {
            omit_info_if_absent: true,
            preserve_info_dates: true,
            ..SaveOptions::default()
        }
    } else {
        write_info_dictionary(doc, &state);
        write_xmp_stream(doc, &state)?;
        SaveOptions {
            omit_info_if_absent: state.is_empty(),
            preserve_info_dates: true,
            ..SaveOptions::default()
        }
    };

    Ok(vec![
        OutputDoc::from_pdf(naming::default_pdf_output_name(&stem, "pdf.metadata"), pdf)
            .with_save_options(save_options),
    ])
}

fn has_any_metadata_field(fields: &MetadataFields) -> bool {
    fields.title.is_some()
        || fields.author.is_some()
        || fields.subject.is_some()
        || fields.keywords.is_some()
        || fields.creator.is_some()
        || fields.producer.is_some()
        || fields.created.is_some()
        || fields.modified.is_some()
        || !fields.custom.is_empty()
}

fn read_existing_metadata(pdf: &PdfDoc, ctx: &ToolCtx) -> Result<MetadataState, PdfError> {
    let report = info::from_doc_with_ctx(pdf, ctx)?;
    let doc = pdf.inner();
    let mut state = MetadataState {
        title: report.metadata.title,
        author: report.metadata.author,
        subject: None,
        keywords: None,
        creator: report.metadata.creator,
        producer: report.metadata.producer,
        created: report.metadata.creation_date,
        modified: report.metadata.mod_date,
        language: report.metadata.language,
        custom: BTreeMap::new(),
    };

    if let Some(info_dict) = resolve_info_dict(doc) {
        state.subject = dict_string(info_dict, b"Subject");
        state.keywords = dict_string(info_dict, b"Keywords");
        for (key, value) in info_dict.iter() {
            if STANDARD_KEYS.contains(&key.as_slice()) {
                continue;
            }
            if let Ok(key_str) = std::str::from_utf8(key)
                && let Some(val_str) = object_to_string(value)
            {
                state.custom.insert(key_str.to_owned(), val_str);
            }
        }
    }

    Ok(state)
}

fn resolve_info_dict(doc: &lopdf::Document) -> Option<&Dictionary> {
    let info_obj = doc.trailer.get(b"Info").ok()?;
    match info_obj {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok(),
        Object::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

fn dict_string(dict: &Dictionary, key: &[u8]) -> Option<String> {
    dict.get(key).ok().and_then(object_to_string)
}

fn object_to_string(obj: &Object) -> Option<String> {
    match obj {
        Object::String(bytes, _) => {
            if bytes.starts_with(&[0xFE, 0xFF]) {
                let utf16: Vec<u16> = bytes[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(u16::from_be_bytes)
                    .collect();
                String::from_utf16(&utf16).ok()
            } else {
                String::from_utf8(bytes.clone()).ok()
            }
        }
        Object::Name(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

fn apply_remove_key(state: &mut MetadataState, key: &str) {
    match key.to_ascii_lowercase().as_str() {
        "title" => state.title = None,
        "author" => state.author = None,
        "subject" | "description" => state.subject = None,
        "keywords" => state.keywords = None,
        "creator" => state.creator = None,
        "producer" => state.producer = None,
        "created" | "creationdate" | "creation_date" => state.created = None,
        "modified" | "moddate" | "mod_date" => state.modified = None,
        "language" | "lang" => state.language = None,
        _ => {
            state.custom.remove(key);
        }
    }
}

fn apply_set_fields(state: &mut MetadataState, fields: &MetadataFields) {
    if let Some(title) = &fields.title {
        state.title = Some(title.clone());
    }
    if let Some(author) = &fields.author {
        state.author = Some(author.clone());
    }
    if let Some(subject) = &fields.subject {
        state.subject = Some(subject.clone());
    }
    if let Some(keywords) = &fields.keywords {
        state.keywords = Some(keywords.clone());
    }
    if let Some(creator) = &fields.creator {
        state.creator = Some(creator.clone());
    }
    if let Some(producer) = &fields.producer {
        state.producer = Some(producer.clone());
    }
    if let Some(created) = &fields.created {
        state.created = Some(created.clone());
    }
    if let Some(modified) = &fields.modified {
        state.modified = Some(modified.clone());
    }
    for (key, value) in &fields.custom {
        state.custom.insert(key.clone(), value.clone());
    }
}

fn remove_info_and_xmp(doc: &mut lopdf::Document) -> Result<(), PdfError> {
    if let Ok(Object::Reference(info_id)) = doc.trailer.get(b"Info").cloned() {
        doc.objects.remove(&info_id);
    }
    doc.trailer.remove(b"Info");

    let catalog_id = catalog_object_id(doc)?;
    if let Ok(Object::Dictionary(catalog)) = doc.get_object_mut(catalog_id) {
        let meta_ref = catalog
            .get(b"Metadata")
            .ok()
            .and_then(|o| o.as_reference().ok());
        catalog.remove(b"Metadata");
        if let Some(meta_id) = meta_ref {
            doc.objects.remove(&meta_id);
        }
    }
    Ok(())
}

fn write_info_dictionary(doc: &mut lopdf::Document, state: &MetadataState) {
    let mut dict = Dictionary::new();
    set_opt_literal(&mut dict, "Title", state.title.as_deref());
    set_opt_literal(&mut dict, "Author", state.author.as_deref());
    set_opt_literal(&mut dict, "Subject", state.subject.as_deref());
    set_opt_literal(&mut dict, "Keywords", state.keywords.as_deref());
    set_opt_literal(&mut dict, "Creator", state.creator.as_deref());
    set_opt_literal(&mut dict, "Producer", state.producer.as_deref());
    set_opt_literal(&mut dict, "CreationDate", state.created.as_deref());
    set_opt_literal(&mut dict, "ModDate", state.modified.as_deref());
    for (key, value) in &state.custom {
        dict.set(
            key.as_bytes().to_vec(),
            Object::String(value.as_bytes().to_vec(), StringFormat::Literal),
        );
    }

    if let Ok(Object::Reference(info_id)) = doc.trailer.get(b"Info").cloned()
        && doc.objects.contains_key(&info_id)
    {
        doc.objects.insert(info_id, Object::Dictionary(dict));
    } else {
        let info_id = doc.add_object(Object::Dictionary(dict));
        doc.trailer.set("Info", Object::Reference(info_id));
    }
}

fn set_opt_literal(dict: &mut Dictionary, key: &str, value: Option<&str>) {
    if let Some(val) = value {
        dict.set(
            key,
            Object::String(val.as_bytes().to_vec(), StringFormat::Literal),
        );
    }
}

fn write_xmp_stream(doc: &mut lopdf::Document, state: &MetadataState) -> Result<(), PdfError> {
    let xml = build_xmp_packet(state);
    let stream = Stream::new(
        dictionary! {
            "Type" => "Metadata",
            "Subtype" => "XML",
        },
        xml.into_bytes(),
    );

    let catalog_id = catalog_object_id(doc)?;
    let existing_meta_id = doc
        .get_object(catalog_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Metadata").ok())
        .and_then(|o| o.as_reference().ok());

    let meta_id = if let Some(id) = existing_meta_id
        && doc.objects.contains_key(&id)
    {
        doc.objects.insert(id, Object::Stream(stream));
        id
    } else {
        doc.add_object(Object::Stream(stream))
    };

    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .as_dict_mut()
        .map_err(|_| PdfError::Malformed)?;
    catalog.set("Metadata", Object::Reference(meta_id));
    Ok(())
}

fn build_xmp_packet(state: &MetadataState) -> String {
    let mut rdf_body = String::new();
    if let Some(title) = &state.title {
        rdf_body.push_str(&format!(
            "      <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>\n",
            escape_xml(title)
        ));
    }
    if let Some(author) = &state.author {
        rdf_body.push_str(&format!(
            "      <dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>\n",
            escape_xml(author)
        ));
    }
    if let Some(subject) = &state.subject {
        rdf_body.push_str(&format!(
            "      <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>\n",
            escape_xml(subject)
        ));
    }
    if let Some(language) = &state.language {
        rdf_body.push_str(&format!(
            "      <dc:language><rdf:Bag><rdf:li>{}</rdf:li></rdf:Bag></dc:language>\n",
            escape_xml(language)
        ));
    }
    if let Some(keywords) = &state.keywords {
        rdf_body.push_str(&format!(
            "      <pdf:Keywords>{}</pdf:Keywords>\n",
            escape_xml(keywords)
        ));
    }
    if let Some(producer) = &state.producer {
        rdf_body.push_str(&format!(
            "      <pdf:Producer>{}</pdf:Producer>\n",
            escape_xml(producer)
        ));
    }
    if let Some(creator) = &state.creator {
        rdf_body.push_str(&format!(
            "      <xmp:CreatorTool>{}</xmp:CreatorTool>\n",
            escape_xml(creator)
        ));
    }
    if let Some(created) = &state.created {
        rdf_body.push_str(&format!(
            "      <xmp:CreateDate>{}</xmp:CreateDate>\n",
            escape_xml(created)
        ));
    }
    if let Some(modified) = &state.modified {
        rdf_body.push_str(&format!(
            "      <xmp:ModifyDate>{}</xmp:ModifyDate>\n",
            escape_xml(modified)
        ));
    }

    format!(
        "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         \x20 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         \x20   <rdf:Description rdf:about=\"\"\n\
         \x20     xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
         \x20     xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\"\n\
         \x20     xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n\
         {rdf_body}\
         \x20   </rdf:Description>\n\
         \x20 </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"w\"?>"
    )
}

fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn catalog_object_id(doc: &lopdf::Document) -> Result<ObjectId, PdfError> {
    doc.trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| PdfError::Malformed)
}
