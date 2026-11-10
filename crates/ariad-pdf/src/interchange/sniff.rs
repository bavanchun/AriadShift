//! Conservative detection of supported form-data formats.

use super::check_data_size;
use crate::PdfError;

/// Recognized interchange formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFormat {
    Fdf,
    Xfdf,
    Json,
    Csv,
}

/// Detect a UTF-8 data format without reading any referenced resource.
pub fn sniff(bytes: &[u8]) -> Result<DataFormat, PdfError> {
    check_data_size(bytes)?;
    // FDF strings may contain arbitrary bytes and are decoded by the FDF parser.
    if bytes.starts_with(b"%FDF-") {
        return Ok(DataFormat::Fdf);
    }
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) || bytes.contains(&0) {
        return Err(PdfError::InvalidParams(
            "interchange input must be UTF-8".into(),
        ));
    }
    let content = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = std::str::from_utf8(content)
        .map_err(|_| PdfError::InvalidParams("interchange input must be UTF-8".into()))?
        .trim_start();
    if text.starts_with('{') || text.starts_with('[') {
        return Ok(DataFormat::Json);
    }
    if text.starts_with('<') {
        use quick_xml::events::Event;
        let mut reader = quick_xml::Reader::from_str(text);
        loop {
            match reader
                .read_event()
                .map_err(|_| PdfError::InvalidParams("invalid XML input".into()))?
            {
                Event::Start(root) | Event::Empty(root) => {
                    return if root.local_name().as_ref() == "xfdf" {
                        Ok(DataFormat::Xfdf)
                    } else {
                        Ok(DataFormat::Csv)
                    };
                }
                Event::DocType(_) => {
                    return Err(PdfError::InvalidParams("DTD is not allowed in XFDF".into()));
                }
                Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
                Event::Text(text) if text.xml10_content().trim().is_empty() => {}
                _ => return Err(PdfError::InvalidParams("invalid XML input".into())),
            }
        }
    }
    Ok(DataFormat::Csv)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_formats_and_rejects_utf16() {
        assert_eq!(sniff(b"%FDF-1.2").unwrap(), DataFormat::Fdf);
        assert_eq!(sniff(b"\xef\xbb\xbf  {\"a\":1}").unwrap(), DataFormat::Json);
        assert_eq!(
            sniff(b"  <?xml version=\"1.0\"?><xfdf/>").unwrap(),
            DataFormat::Xfdf
        );
        assert_eq!(sniff(b"a,b\n1,2\n").unwrap(), DataFormat::Csv);
        assert!(sniff(&[0xff, 0xfe, 0x61, 0]).is_err());
    }
}
