//! Enforce caller resource budgets before allocating parsed interchange values.
use ariad_core::{limits::Limits, reader::pdf::ReadError};
use quick_xml::{Reader, events::Event};

use super::{check_data_size, sniff::DataFormat};
use crate::{PdfError, ToolCtx};

fn blocks(limit: u32) -> PdfError {
    PdfError::LimitExceeded(ReadError::TooManyBlocks { limit })
}
fn depth(limit: u16) -> PdfError {
    PdfError::LimitExceeded(ReadError::NestingTooDeep { limit })
}

pub(super) fn check(
    bytes: &[u8],
    format: DataFormat,
    delimiter: Option<char>,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    check_data_size(bytes)?;
    ctx.check()?;
    match format {
        DataFormat::Fdf => super::fdf::check_limits(bytes, limits, ctx),
        DataFormat::Json => {
            let mut scanner =
                ariad_core::json_depth::JsonDepthScanner::new(limits.max_nesting_depth as usize);
            let mut in_string = false;
            let mut escaped = false;
            let mut primitive = false;
            let mut count = 0u32;
            for chunk in bytes
                .strip_prefix(b"\xef\xbb\xbf")
                .unwrap_or(bytes)
                .chunks(4096)
            {
                ctx.check()?;
                scanner
                    .feed(chunk)
                    .map_err(|_| depth(limits.max_nesting_depth))?;
                for &c in chunk {
                    if in_string {
                        if escaped {
                            escaped = false;
                        } else if c == b'\\' {
                            escaped = true;
                        } else if c == b'"' {
                            in_string = false;
                        }
                    } else {
                        match c {
                            b'"' => {
                                in_string = true;
                                primitive = false;
                                count = count.saturating_add(1);
                            }
                            b'{' | b'[' => {
                                primitive = false;
                                count = count.saturating_add(1);
                            }
                            b'}' | b']' | b':' | b',' | b' ' | b'\r' | b'\n' | b'\t' => {
                                primitive = false
                            }
                            _ if !primitive => {
                                primitive = true;
                                count = count.saturating_add(1);
                            }
                            _ => {}
                        }
                        if count > limits.max_blocks {
                            return Err(blocks(limits.max_blocks));
                        }
                    }
                }
            }
            Ok(())
        }
        DataFormat::Xfdf => {
            let mut reader = Reader::from_reader(bytes);
            reader.config_mut().expand_empty_elements = true;
            let mut count = 0u32;
            let mut level = 0u16;
            loop {
                let event = reader
                    .read_event()
                    .map_err(|_| PdfError::InvalidParams("invalid XFDF input".into()))?;
                if matches!(event, Event::Eof) {
                    break;
                }
                count = count.saturating_add(1);
                if count > limits.max_blocks {
                    return Err(blocks(limits.max_blocks));
                }
                if count % 256 == 0 {
                    ctx.check()?;
                }
                match event {
                    Event::Start(_) => {
                        level = level.saturating_add(1);
                        if level > limits.max_nesting_depth {
                            return Err(depth(limits.max_nesting_depth));
                        }
                    }
                    Event::End(_) => level = level.saturating_sub(1),
                    _ => {}
                }
            }
            Ok(())
        }
        DataFormat::Csv => {
            let delimiter = match delimiter {
                Some(c) if c.is_ascii() && !matches!(c, '\r' | '\n' | '"' | '\0') => c as u8,
                Some(_) => return Err(PdfError::InvalidParams("invalid CSV delimiter".into())),
                None => super::csv::detect_delimiter(
                    bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes),
                ),
            };
            let mut reader = csv::ReaderBuilder::new()
                .delimiter(delimiter)
                .has_headers(false)
                .flexible(true)
                .from_reader(bytes);
            let mut count = 0u32;
            for row in reader.byte_records() {
                ctx.check()?;
                let row = row.map_err(|_| PdfError::InvalidParams("invalid CSV input".into()))?;
                count = count.saturating_add(u32::try_from(row.len()).unwrap_or(u32::MAX));
                if count > limits.max_blocks {
                    return Err(blocks(limits.max_blocks));
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_budgets_apply_to_each_data_format() {
        let ctx = ToolCtx::new();
        let mut limits = Limits::local();
        limits.max_blocks = 2;
        for (format, input) in [
            (DataFormat::Json, &b"{\"a\":1,\"b\":2}"[..]),
            (DataFormat::Csv, &b"a,b\n1,2\n"[..]),
            (DataFormat::Xfdf, &b"<x><a/><b/></x>"[..]),
            (
                DataFormat::Fdf,
                &b"%FDF-1.2\n1 0 obj << /FDF << >> >> endobj\n%%EOF"[..],
            ),
        ] {
            assert!(matches!(
                check(input, format, None, &limits, &ctx),
                Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: 2
                }))
            ));
        }
        limits.max_blocks = 100;
        limits.max_nesting_depth = 1;
        for (format, input) in [
            (DataFormat::Json, &b"{\"a\":[]}"[..]),
            (DataFormat::Xfdf, &b"<x><a/></x>"[..]),
            (
                DataFormat::Fdf,
                &b"%FDF-1.2\n1 0 obj << /a [] >> endobj\n%%EOF"[..],
            ),
        ] {
            assert!(matches!(
                check(input, format, None, &limits, &ctx),
                Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                    limit: 1
                }))
            ));
        }
    }
}
