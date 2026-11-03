//! Bounded JSON form-data import and export.

use super::MAX_DATA_STRING_BYTES;

use std::fmt;

use ariad_core::{
    json_depth::prescan_json_depth,
    tools::params::form_model::{FormValue, MAX_IMPORT_RECORDS},
};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;

use super::{FormRecord, MAX_DATA_DEPTH, check_data_size};
use crate::{PdfError, ToolCtx};

struct Record {
    values: FormRecord,
    duplicate: bool,
}
impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor;
        impl<'de> Visitor<'de> for RecordVisitor {
            type Value = Record;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a form data object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Record, A::Error> {
                let mut values = FormRecord::new();
                let mut duplicate = false;
                let mut entries = 0usize;
                while let Some((key, raw)) =
                    map.next_entry::<String, Box<serde_json::value::RawValue>>()?
                {
                    entries += 1;
                    if entries > 50_000 {
                        return Err(de::Error::custom("too many fields"));
                    }
                    if key.len() > MAX_DATA_STRING_BYTES {
                        return Err(de::Error::custom("field name is too long"));
                    }
                    let text = raw.get();
                    let value = if text
                        .as_bytes()
                        .first()
                        .is_some_and(|c| c.is_ascii_digit() || *c == b'-')
                    {
                        if text.len() > MAX_DATA_STRING_BYTES {
                            return Err(de::Error::custom("form value is too long"));
                        }
                        FormValue::Text(text.to_owned())
                    } else {
                        let parsed: Value =
                            serde_json::from_str(text).map_err(de::Error::custom)?;
                        convert(parsed).map_err(de::Error::custom)?
                    };
                    duplicate |= values.insert(key, value).is_some();
                }
                Ok(Record { values, duplicate })
            }
        }
        deserializer.deserialize_map(RecordVisitor)
    }
}
fn convert(value: Value) -> Result<FormValue, &'static str> {
    match value {
        Value::String(s) if s.len() <= MAX_DATA_STRING_BYTES => Ok(FormValue::Text(s)),
        Value::Number(n) => Ok(FormValue::Text(n.to_string())),
        Value::Bool(b) => Ok(FormValue::Bool(b)),
        Value::Null => Ok(FormValue::Null(())),
        Value::Array(v) => {
            let mut values = Vec::with_capacity(v.len());
            let mut total = 0usize;
            for value in v {
                let Value::String(s) = value else {
                    return Err("array values must be strings");
                };
                total = total.saturating_add(s.len());
                if total > MAX_DATA_STRING_BYTES {
                    return Err("form value is too long");
                }
                values.push(s);
            }
            Ok(FormValue::Multi(values))
        }
        _ => Err("invalid form value"),
    }
}

struct Records<'a> {
    max: usize,
    ctx: &'a ToolCtx,
}
impl<'de> de::DeserializeSeed<'de> for Records<'_> {
    type Value = Vec<Record>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        struct RecordsVisitor<'a> {
            max: usize,
            ctx: &'a ToolCtx,
        }
        impl<'de> Visitor<'de> for RecordsVisitor<'_> {
            type Value = Vec<Record>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a form object or array of objects")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                self.ctx.check().map_err(de::Error::custom)?;
                let record = Record::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(vec![record])
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut records = Vec::new();
                while let Some(record) = seq.next_element::<Record>()? {
                    self.ctx.check().map_err(de::Error::custom)?;
                    if records.len() >= self.max {
                        return Err(de::Error::custom("too many records"));
                    }
                    records.push(record);
                }
                Ok(records)
            }
        }
        deserializer.deserialize_any(RecordsVisitor {
            max: self.max,
            ctx: self.ctx,
        })
    }
}

/// Parse one JSON form record or a batch; duplicate names keep the final value.
pub fn read_forms(
    bytes: &[u8],
    max_records: usize,
    ctx: &ToolCtx,
) -> Result<(Vec<FormRecord>, Vec<String>), PdfError> {
    use serde::de::DeserializeSeed;
    ctx.check()?;
    check_data_size(bytes)?;
    if max_records == 0 || max_records > MAX_IMPORT_RECORDS {
        return Err(PdfError::InvalidParams(
            "invalid input size or record limit".into(),
        ));
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    prescan_json_depth(bytes, MAX_DATA_DEPTH)
        .map_err(|_| PdfError::InvalidParams("JSON nesting limit exceeded".into()))?;
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let parsed = Records {
        max: max_records,
        ctx,
    }
    .deserialize(&mut parser);
    // Preserve cancellation and timeout errors instead of masking them as syntax errors.
    ctx.check()?;
    let parsed = parsed.map_err(|_| PdfError::InvalidParams("invalid form JSON".into()))?;
    parser
        .end()
        .map_err(|_| PdfError::InvalidParams("invalid form JSON".into()))?;
    let duplicate = parsed.iter().any(|r| r.duplicate);
    let warnings = if duplicate {
        vec!["duplicate_key: duplicate field name; last value used".to_owned()]
    } else {
        Vec::new()
    };
    Ok((parsed.into_iter().map(|r| r.values).collect(), warnings))
}

/// Serialize one form record in deterministic field-name order.
pub fn write_forms(record: &FormRecord) -> Result<Vec<u8>, PdfError> {
    let mut output = super::BoundedWriter::default();
    serde_json::to_writer(&mut output, record)
        .map_err(|_| PdfError::InvalidParams("form JSON exceeds the output limit".into()))?;
    Ok(output.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_preserve_types_and_report_duplicates() {
        let input = br#"[{"x":1,"x":2,"ok":false,"v":["a","b"],"clear":null}]"#;
        let (records, warnings) = read_forms(input, 1, &ToolCtx::new()).unwrap();
        assert_eq!(records[0]["x"], FormValue::Text("2".into()));
        assert_eq!(records[0]["ok"], FormValue::Bool(false));
        assert_eq!(warnings.len(), 1);
        let encoded = write_forms(&records[0]).unwrap();
        assert_eq!(
            read_forms(&encoded, 1, &ToolCtx::new()).unwrap().0[0],
            records[0]
        );
    }
    #[test]
    fn rejects_invalid_values_and_excess_records() {
        let (records, _) = read_forms(br#"{"number":1.2500e+4}"#, 1, &ToolCtx::new()).unwrap();
        assert_eq!(records[0]["number"], FormValue::Text("1.2500e+4".into()));
        assert!(read_forms(br#"[{},{ }]"#, 1, &ToolCtx::new()).is_err());
        assert!(read_forms(br#"{"x":{"nested":1}}"#, 1, &ToolCtx::new()).is_err());
        let deeply_nested = format!("{}0{}", "[".repeat(65), "]".repeat(65));
        assert!(read_forms(deeply_nested.as_bytes(), 1, &ToolCtx::new()).is_err());
    }
}

/// Read annotation metadata exported by AriadShift, rejecting unsupported or invalid entries.
pub fn read_annotations(
    bytes: &[u8],
    ctx: &ToolCtx,
) -> Result<Vec<ariad_core::tools::params::annotations::AnnotationItem>, PdfError> {
    use ariad_core::tools::params::annotations::{AnnotationInfo, MAX_ANNOTATION_ITEMS};
    check_data_size(bytes)?;
    ctx.check()?;
    prescan_json_depth(bytes, MAX_DATA_DEPTH)
        .map_err(|_| PdfError::InvalidParams("annotation JSON exceeds nesting limit".into()))?;
    #[derive(Deserialize)]
    struct Data {
        annotations: Vec<AnnotationInfo>,
    }
    let data: Data = serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes))
        .map_err(|_| PdfError::InvalidParams("invalid annotation JSON".into()))?;
    if data.annotations.len() > MAX_ANNOTATION_ITEMS {
        return Err(PdfError::InvalidParams("too many annotations".into()));
    }
    let mut items = Vec::with_capacity(data.annotations.len());
    for info in data.annotations {
        ctx.check()?;
        let item = info
            .to_item()
            .ok_or_else(|| PdfError::InvalidParams("unsupported annotation type".into()))?;
        item.validate().map_err(PdfError::InvalidParams)?;
        items.push(item);
    }
    Ok(items)
}
