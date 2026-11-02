//! Serialize the inspected object graph for PDFium without preserving input xrefs.
use std::io::{self, Write};

use ariad_core::protocol::ErrorCode;
use ariad_engine_io::EngineFailure;
use ariad_pdf::PdfDoc;

use crate::output::ToolContext;

struct BoundedWriter<'a, 'b> {
    bytes: Vec<u8>,
    limit: usize,
    ctx: &'a ToolContext<'b>,
}

impl Write for BoundedWriter<'_, '_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.ctx
            .check()
            .map_err(|_| io::Error::other("interrupted"))?;
        let len = self
            .bytes
            .len()
            .checked_add(buf.len())
            .ok_or_else(|| io::Error::other("limit"))?;
        if len > self.limit {
            return Err(io::Error::other("limit"));
        }
        if len > self.bytes.capacity() {
            let capacity = len
                .max(self.bytes.capacity().saturating_mul(2))
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::other("limit"))?;
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_name(name: &[u8]) -> Result<(), EngineFailure> {
    if name.is_empty() {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "PDF contains an empty name.",
        ));
    }
    let encoded = name.iter().fold(0usize, |len, &byte| {
        len.saturating_add(
            if b" \t\n\r\x0c()<>[]{}/%#".contains(&byte) || !(33..=126).contains(&byte) {
                3
            } else {
                1
            },
        )
    });
    if encoded > 254 {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "PDF object name exceeds the safe token length.",
        ));
    }
    Ok(())
}

// Lopdf's literal-string writer can do quadratic work on unmatched parentheses.
// Hex strings preserve the value and are written in linear time.
pub(crate) fn normalize_strings(
    ctx: &ToolContext<'_>,
    object: &mut lopdf::Object,
    depth: usize,
    max_depth: usize,
) -> Result<(), EngineFailure> {
    ctx.check()?;
    if depth >= 63 || depth > max_depth {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "PDF object nesting exceeds the safe depth.",
        ));
    }
    match object {
        lopdf::Object::String(_, format) => *format = lopdf::StringFormat::Hexadecimal,
        lopdf::Object::Integer(value) if i32::try_from(*value).is_err() => {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "PDF integer exceeds the safe range.",
            ));
        }
        lopdf::Object::Real(value) if !value.is_finite() => {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "PDF contains a non-finite real number.",
            ));
        }
        lopdf::Object::Name(name) => validate_name(name)?,

        lopdf::Object::Array(items) => {
            for item in items {
                normalize_strings(ctx, item, depth + 1, max_depth)?;
            }
        }
        lopdf::Object::Dictionary(dict) => {
            for (name, item) in dict.iter_mut() {
                validate_name(name)?;
                normalize_strings(ctx, item, depth + 1, max_depth)?;
            }
        }
        lopdf::Object::Stream(stream) => {
            for (name, item) in stream.dict.iter_mut() {
                validate_name(name)?;
                normalize_strings(ctx, item, depth + 1, max_depth)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_references(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    object: &lopdf::Object,
    depth: usize,
    max_depth: usize,
) -> Result<(), EngineFailure> {
    ctx.check()?;
    if depth >= 63 || depth > max_depth {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "PDF object nesting exceeds the safe depth.",
        ));
    }
    match object {
        lopdf::Object::Reference(id) => {
            if id.0 == 0 {
                return Err(EngineFailure::new(
                    ErrorCode::LimitExceeded,
                    "PDF refers to object zero.",
                ));
            }
            let other_generation = doc.objects.range((id.0, 0)..=(id.0, u16::MAX)).next();
            if other_generation.is_some_and(|(actual, object)| {
                actual != id
                    || matches!(object.type_name(), Ok(b"ObjStm" | b"XRef" | b"Linearized"))
            }) {
                return Err(EngineFailure::new(
                    ErrorCode::LimitExceeded,
                    "PDF contains an unresolved object reference.",
                ));
            }
        }
        lopdf::Object::Array(items) => {
            for item in items {
                validate_references(ctx, doc, item, depth + 1, max_depth)?;
            }
        }
        lopdf::Object::Dictionary(dict) => {
            for (_, item) in dict.iter() {
                validate_references(ctx, doc, item, depth + 1, max_depth)?;
            }
        }
        lopdf::Object::Stream(stream) => {
            for (_, item) in stream.dict.iter() {
                validate_references(ctx, doc, item, depth + 1, max_depth)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn canonicalize(
    ctx: &ToolContext<'_>,
    doc: &mut PdfDoc,
    input_len: usize,
) -> Result<Vec<u8>, EngineFailure> {
    let limit = doc.remaining_decode_bytes().min(
        ctx.limits()
            .max_memory_mb
            .map_or(u64::MAX, |m| u64::from(m).saturating_mul(1024 * 1024)),
    );
    let limit = usize::try_from(limit)
        .unwrap_or(usize::MAX)
        .min(u32::MAX as usize)
        .checked_sub(input_len)
        .ok_or_else(EngineFailure::limit_exceeded)?;
    crate::extract_images::preflight_document_content(ctx, doc)?;
    let content_streams = crate::extract_images::canonical_content_streams(ctx, doc)?;
    let inner = doc
        .inner_mut()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    if !matches!(
        inner.version.as_str(),
        "1.0" | "1.1" | "1.2" | "1.3" | "1.4" | "1.5" | "1.6" | "1.7" | "2.0"
    ) {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "PDF version cannot be serialized safely.",
        ));
    }
    if inner.max_id > 1_000_000 || inner.objects.keys().any(|&(number, _)| number > 1_000_000) {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "PDF object numbers exceed the safe range.",
        ));
    }
    let mut last = None;
    for &(number, _) in inner.objects.keys() {
        if number == 0 || last == Some(number) {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "PDF contains conflicting object numbers.",
            ));
        }
        last = Some(number);
    }
    // Lopdf writes the dictionary as-is; make its length describe the exact
    // content that will be serialized, including formerly indirect lengths.
    for object in inner.objects.values_mut() {
        ctx.check()?;
        if let lopdf::Object::Stream(stream) = object {
            let length =
                i64::try_from(stream.content.len()).map_err(|_| EngineFailure::limit_exceeded())?;
            stream.dict.set("Length", length);
        }
    }
    let max_depth = usize::from(ctx.limits().max_nesting_depth);
    for object in inner.objects.values_mut() {
        normalize_strings(ctx, object, 0, max_depth)?;
    }
    for (name, value) in inner.trailer.iter_mut() {
        validate_name(name)?;
        normalize_strings(ctx, value, 0, max_depth)?;
    }
    for object in inner.objects.values() {
        ctx.check()?;
        validate_references(ctx, inner, object, 0, max_depth)?;
    }
    inner.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    inner.trailer.remove(b"Prev");
    inner.trailer.remove(b"XRefStm");
    validate_references(
        ctx,
        inner,
        &lopdf::Object::Dictionary(inner.trailer.clone()),
        0,
        max_depth,
    )?;
    let mut out = BoundedWriter {
        bytes: Vec::new(),
        limit,
        ctx,
    };
    if write_document(&mut out, inner, &content_streams).is_err() {
        ctx.check()?;
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "Canonical PDF output exceeds the safe write budget.",
        ));
    }
    ctx.check()?;
    check_round_trip_with_streams(inner, &out.bytes, &content_streams)?;
    ctx.check()?;
    Ok(out.bytes)
}

// Writer for the subset of PDF syntax consumed by both parsers.
fn write_name(out: &mut impl Write, name: &[u8]) -> io::Result<()> {
    out.write_all(b"/")?;
    for &byte in name {
        if b" \t\n\r\x0c()<>[]{}/%#".contains(&byte) || !(33..=126).contains(&byte) {
            write!(out, "#{byte:02X}")?;
        } else {
            out.write_all(&[byte])?;
        }
    }
    Ok(())
}

pub(crate) fn write_object(out: &mut impl Write, object: &lopdf::Object) -> io::Result<()> {
    use lopdf::Object;
    match object {
        Object::Null => out.write_all(b"null"),
        Object::Boolean(v) => out.write_all(if *v { b"true" } else { b"false" }),
        Object::Integer(v) => write!(out, "{v}"),
        Object::Real(v) => {
            let mut value = v.to_string();
            if !value.contains('.') {
                value.push_str(".0");
            }
            out.write_all(value.as_bytes())
        }
        Object::Name(name) => write_name(out, name),
        Object::String(bytes, _) => {
            out.write_all(b"<")?;
            for byte in bytes {
                write!(out, "{byte:02X}")?;
            }
            out.write_all(b">")
        }
        Object::Array(items) => {
            out.write_all(b"[")?;
            for item in items {
                write_object(out, item)?;
                out.write_all(b" ")?;
            }
            out.write_all(b"]")
        }
        Object::Dictionary(dict) => write_dictionary(out, dict),
        Object::Stream(stream) => {
            write_dictionary(out, &stream.dict)?;
            out.write_all(b"\nstream\n")?;
            out.write_all(&stream.content)?;
            out.write_all(b"\nendstream")
        }
        Object::Reference((number, generation)) => write!(out, "{number} {generation} R"),
    }
}

fn write_dictionary(out: &mut impl Write, dict: &lopdf::Dictionary) -> io::Result<()> {
    out.write_all(b"<<")?;
    for (name, value) in dict.iter() {
        write_name(out, name)?;
        out.write_all(b" ")?;
        write_object(out, value)?;
        out.write_all(b" ")?;
    }
    out.write_all(b">>")
}

fn same_object(a: &lopdf::Object, b: &lopdf::Object) -> bool {
    use lopdf::Object;
    match (a, b) {
        (Object::Stream(a), Object::Stream(b)) => {
            a.content == b.content && same_dict(&a.dict, &b.dict)
        }
        (Object::Dictionary(a), Object::Dictionary(b)) => same_dict(a, b),
        (Object::Array(a), Object::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_object(a, b))
        }
        (Object::String(a, _), Object::String(b, _)) => a == b,
        (Object::Real(a), Object::Real(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

fn same_dict(a: &lopdf::Dictionary, b: &lopdf::Dictionary) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(key, value)| b.get(key).is_ok_and(|other| same_object(value, other)))
}

fn is_regenerated(object: &lopdf::Object) -> bool {
    matches!(object.type_name(), Ok(b"ObjStm" | b"XRef" | b"Linearized"))
}

fn write_document(
    out: &mut BoundedWriter<'_, '_>,
    doc: &mut lopdf::Document,
    content_streams: &std::collections::BTreeMap<lopdf::ObjectId, lopdf::Object>,
) -> io::Result<()> {
    writeln!(out, "%PDF-{}", doc.version)?;
    out.write_all(b"%\xE2\xE3\xCF\xD3\n")?;
    let mut offsets = Vec::new();
    for (&(number, generation), object) in &doc.objects {
        if is_regenerated(object) {
            continue;
        }
        offsets.push((number, generation, out.bytes.len()));
        writeln!(out, "{number} {generation} obj")?;
        write_object(
            out,
            content_streams.get(&(number, generation)).unwrap_or(object),
        )?;
        out.write_all(b"\nendobj\n")?;
    }
    let start = out.bytes.len();
    out.write_all(b"xref\n0 1\n0000000000 65535 f \n")?;
    for (number, generation, offset) in offsets {
        writeln!(out, "{number} 1\n{offset:010} {generation:05} n ")?;
    }
    doc.trailer.set("Size", i64::from(doc.max_id) + 1);
    out.write_all(b"trailer\n")?;
    write_dictionary(out, &doc.trailer)?;
    write!(out, "\nstartxref\n{start}\n%%EOF")
}

#[cfg(test)]
fn check_round_trip(expected: &lopdf::Document, bytes: &[u8]) -> Result<(), EngineFailure> {
    check_round_trip_with_streams(expected, bytes, &std::collections::BTreeMap::new())
}

fn check_round_trip_with_streams(
    expected: &lopdf::Document,
    bytes: &[u8],
    content_streams: &std::collections::BTreeMap<lopdf::ObjectId, lopdf::Object>,
) -> Result<(), EngineFailure> {
    let failure = || {
        EngineFailure::new(
            ErrorCode::LimitExceeded,
            "Canonical PDF does not preserve the inspected object graph.",
        )
    };
    let actual = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            strict: true,
            ..Default::default()
        },
    )
    .map_err(|_| failure())?;
    if actual.xref_recovery.is_some() || !same_dict(&expected.trailer, &actual.trailer) {
        return Err(failure());
    }
    let expected_count = expected
        .objects
        .values()
        .filter(|object| !is_regenerated(object))
        .count();
    if expected_count != actual.objects.len() {
        return Err(failure());
    }
    for (id, object) in &expected.objects {
        if !is_regenerated(object)
            && !actual
                .objects
                .get(id)
                .is_some_and(|other| same_object(content_streams.get(id).unwrap_or(object), other))
        {
            return Err(failure());
        }
    }
    Ok(())
}

// Lopdf interprets /Prev 0 as an xref at the header. PDF readers treat it as
// the end of the chain. Remove only that dictionary entry, preserving offsets.
pub(crate) fn normalize_zero_prev(
    ctx: &ToolContext<'_>,
    bytes: &mut [u8],
) -> Result<(), EngineFailure> {
    if !bytes.windows(5).any(|w| w == b"/Prev") {
        return Ok(());
    }
    let Some(header) = bytes.windows(5).position(|w| w == b"%PDF-") else {
        return Ok(());
    };
    let Some(start) = bytes.windows(9).rposition(|w| w == b"startxref") else {
        return Ok(());
    };
    let mut cursor = start + 9;
    let Some((a, b)) = crate::extract_images::next_content_token(bytes, &mut cursor)? else {
        return Ok(());
    };
    let Some(offset) = std::str::from_utf8(&bytes[a..b])
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    else {
        return Ok(());
    };
    let mut pending = vec![offset];
    let mut seen = std::collections::HashSet::new();
    let mut work = 0usize;
    while let Some(offset) = pending.pop() {
        ctx.check()?;
        if !seen.insert(offset) {
            continue;
        }
        if seen.len() > 1024 {
            return Err(EngineFailure::limit_exceeded());
        }
        let Some(mut pos) = header.checked_add(offset).filter(|&n| n < bytes.len()) else {
            break;
        };
        let begin = pos;
        let Some((a, b)) = crate::extract_images::next_content_token(bytes, &mut pos)? else {
            break;
        };
        if &bytes[a..b] == b"xref" {
            loop {
                let Some((a, b)) = crate::extract_images::next_content_token(bytes, &mut pos)?
                else {
                    return Ok(());
                };
                if &bytes[a..b] == b"trailer" {
                    break;
                }
                if pos.saturating_sub(begin).saturating_add(work) > bytes.len().saturating_mul(2) {
                    return Err(EngineFailure::limit_exceeded());
                }
            }
        } else {
            for _ in 0..2 {
                if crate::extract_images::next_content_token(bytes, &mut pos)?.is_none() {
                    return Ok(());
                }
            }
        }
        let Some((a, b)) = crate::extract_images::next_content_token(bytes, &mut pos)? else {
            break;
        };
        if &bytes[a..b] != b"<<" {
            break;
        }
        let mut depth = 1usize;
        let mut array = 0usize;
        let mut expect_key = true;
        let mut zeros = Vec::new();
        let mut prev_count = 0;
        let mut closed = false;
        while let Some((a, b)) = crate::extract_images::next_content_token(bytes, &mut pos)? {
            let token = &bytes[a..b];
            if depth == 1 && array == 0 && expect_key {
                if token == b">>" {
                    closed = true;
                    break;
                }
                if !token.starts_with(b"/") {
                    return Ok(());
                }
                let is_prev = token == b"/Prev";
                let is_xref = token == b"/XRefStm";
                if is_prev {
                    prev_count += 1;
                }
                let Some((v, e)) = crate::extract_images::next_content_token(bytes, &mut pos)?
                else {
                    return Ok(());
                };
                let value = &bytes[v..e];
                let mut lookahead = pos;
                let is_reference = if value.iter().all(u8::is_ascii_digit) {
                    let generation =
                        crate::extract_images::next_content_token(bytes, &mut lookahead)?;
                    let marker = crate::extract_images::next_content_token(bytes, &mut lookahead)?;
                    generation.zip(marker).is_some_and(|((g, h), (r, t))| {
                        bytes[g..h].iter().all(u8::is_ascii_digit) && &bytes[r..t] == b"R"
                    })
                } else {
                    false
                };
                if is_reference {
                    pos = lookahead;
                }
                if !is_reference
                    && (is_prev || is_xref)
                    && value.iter().all(u8::is_ascii_digit)
                    && let Some(n) = std::str::from_utf8(value)
                        .ok()
                        .and_then(|s| s.parse::<usize>().ok())
                {
                    if n == 0 && is_prev {
                        zeros.push((a, e));
                    } else if n > 0 {
                        pending.push(n);
                    }
                }
                match value {
                    b"<<" => {
                        depth += 1;
                        expect_key = false;
                    }
                    b"[" => {
                        array += 1;
                        expect_key = false;
                    }
                    _ => {
                        expect_key = true;
                    }
                }
            } else {
                match token {
                    b"<<" => depth += 1,
                    b">>" => {
                        depth = depth.saturating_sub(1);
                    }
                    b"[" => array += 1,
                    b"]" => {
                        array = array.saturating_sub(1);
                    }
                    _ => {}
                }
                if depth == 1 && array == 0 {
                    expect_key = true;
                }
            }
            if depth > 64
                || pos.saturating_sub(begin).saturating_add(work) > bytes.len().saturating_mul(2)
            {
                return Err(EngineFailure::limit_exceeded());
            }
        }
        work = work.saturating_add(pos.saturating_sub(begin));
        if closed && prev_count == 1 {
            for (a, b) in zeros {
                bytes[a..b].fill(b' ');
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::limits::Limits;
    use ariad_engine_io::RunToolPaths;
    use std::{fs, path::Path, time::Instant};

    #[test]
    fn refuses_excessive_object_numbers() {
        let input = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf/canonical-large-object-number.pdf"),
        )
        .unwrap();
        for stale_max in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let paths = RunToolPaths {
                workspace: dir.path().to_path_buf(),
                inputs: vec![],
                output_dir: dir.path().to_path_buf(),
                work_dir: dir.path().to_path_buf(),
            };
            let mut sink = Vec::new();
            let ctx = ToolContext::new("objects", "pdf.render", paths, Limits::local(), &mut sink);
            let mut doc = ariad_pdf::document::open(&input, ctx.limits()).unwrap();
            let inner = doc.inner_mut().unwrap();
            if stale_max {
                inner.max_id = 3;
            }
            let error = canonicalize(&ctx, &mut doc, input.len()).unwrap_err();
            assert!(
                error.message.contains("object numbers"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn refuses_pdfium_parser_depth_even_with_a_larger_configured_limit() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new("depth", "pdf.render", paths, Limits::local(), &mut sink);
        let nested =
            |depth| (0..depth).fold(lopdf::Object::Null, |v, _| lopdf::Object::Array(vec![v]));
        assert!(normalize_strings(&ctx, &mut nested(62), 0, 100).is_ok());
        assert!(normalize_strings(&ctx, &mut nested(63), 0, 100).is_err());
    }

    #[test]
    fn refuses_deeply_nested_page_fixture() {
        let input = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/canonical-deep-64.pdf"),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let mut limits = Limits::local();
        limits.max_nesting_depth = 99;
        let ctx = ToolContext::new("depth-fixture", "pdf.render", paths, limits, &mut sink);
        let mut doc = ariad_pdf::document::open(&input, ctx.limits()).unwrap();
        let error = canonicalize(&ctx, &mut doc, input.len()).unwrap_err();
        assert!(error.message.contains("nesting"));
    }

    #[test]
    fn refuses_unsafe_scalar_fixtures() {
        for (name, reason) in [
            ("canonical-inf-real.pdf", "non-finite"),
            ("canonical-big-integer.pdf", "integer"),
            ("canonical-zero-ref.pdf", "object zero"),
        ] {
            let input = fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/pdf")
                    .join(name),
            )
            .unwrap();
            let dir = tempfile::tempdir().unwrap();
            let paths = RunToolPaths {
                workspace: dir.path().to_path_buf(),
                inputs: vec![],
                output_dir: dir.path().to_path_buf(),
                work_dir: dir.path().to_path_buf(),
            };
            let mut sink = Vec::new();
            let ctx = ToolContext::new("tokens", "pdf.render", paths, Limits::local(), &mut sink);
            let mut doc = ariad_pdf::document::open(&input, ctx.limits()).unwrap();
            let error = canonicalize(&ctx, &mut doc, input.len()).unwrap_err();
            assert!(error.message.contains(reason), "{name}: {}", error.message);
        }
    }

    #[test]
    fn names_respect_pdfium_token_limit() {
        assert!(validate_name(&[b'a'; 254]).is_ok());
        assert!(validate_name(&[b'#'; 85]).is_err());
        assert!(validate_name(&[]).is_err());
    }

    #[test]
    fn serializes_reals_names_and_strings_losslessly() {
        let object = lopdf::Object::Array(vec![
            lopdf::Object::Real(1.0),
            lopdf::Object::Real(0.00000001),
            lopdf::Object::Name(vec![b'#', b'/', 0]),
            lopdf::Object::String(vec![0, 255], lopdf::StringFormat::Literal),
        ]);
        let mut bytes = Vec::new();
        write_object(&mut bytes, &object).unwrap();
        assert_eq!(bytes, b"[1.0 0.00000001 /#23#2F#00 <00FF> ]");
    }

    #[test]
    fn bounded_output_rejects_growth_past_budget() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new("limit", "pdf.render", paths, Limits::local(), &mut sink);
        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            limit: 4,
            ctx: &ctx,
        };
        writer.write_all(b"1234").unwrap();
        assert!(writer.write_all(b"5").is_err());
        assert_eq!(writer.bytes, b"1234");
    }

    #[test]
    fn literal_strings_are_written_in_linear_time_form() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new("strings", "pdf.render", paths, Limits::local(), &mut sink);
        let mut empty_name = lopdf::Object::Name(Vec::new());
        assert!(normalize_strings(&ctx, &mut empty_name, 0, 64).is_err());
        let mut object = lopdf::Object::String(vec![b'('; 4096], lopdf::StringFormat::Literal);
        normalize_strings(&ctx, &mut object, 0, 64).unwrap();
        assert!(matches!(
            object,
            lopdf::Object::String(_, lopdf::StringFormat::Hexadecimal)
        ));
    }

    #[test]
    fn writes_one_classic_table_without_object_streams() {
        let input = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf/canonical-objstm-index.pdf"),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new("canonical", "pdf.render", paths, Limits::local(), &mut sink);
        let mut doc = ariad_pdf::document::open(&input, ctx.limits()).unwrap();
        let output = canonicalize(&ctx, &mut doc, input.len()).unwrap();
        assert!(output.starts_with(b"%PDF-"));
        assert_eq!(output.windows(5).filter(|w| *w == b"xref\n").count(), 2); // xref and startxref
        assert!(!output.windows(7).any(|w| w == b"/ObjStm"));
        check_round_trip(doc.inner(), &output).unwrap();
        let mut changed = doc.inner().clone();
        changed.trailer.set("Root", lopdf::Object::Null);
        assert!(check_round_trip(&changed, &output).is_err());
        let parsed = lopdf::Document::load_mem_with_options(
            &output,
            lopdf::LoadOptions {
                strict: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(parsed.xref_recovery.is_none());
        assert!(matches!(
            parsed.reference_table.cross_reference_type,
            lopdf::xref::XrefType::CrossReferenceTable
        ));
        assert!(parsed.trailer.get(b"Prev").is_err());
        assert!(parsed.trailer.get(b"XRefStm").is_err());
        let mut previous = None;
        for (number, _) in parsed.objects.keys() {
            assert_ne!(previous, Some(*number));
            previous = Some(*number);
        }
    }

    #[test]
    #[ignore = "timing benchmark"]
    fn time_canonicalization() {
        let list = std::env::var("ARIAD_BENCH_CORPUS_LIST").unwrap();
        let paths = fs::read_to_string(list).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = tempfile::tempdir().unwrap();
        let mut baseline = std::time::Duration::ZERO;
        let mut load = std::time::Duration::ZERO;
        let mut write = std::time::Duration::ZERO;
        let mut count = 0;
        let mut accepted = 0;
        for path in paths.lines() {
            count += 1;
            let paths = RunToolPaths {
                workspace: dir.path().to_path_buf(),
                inputs: vec![root.join(path)],
                output_dir: dir.path().to_path_buf(),
                work_dir: dir.path().to_path_buf(),
            };
            let mut sink = Vec::new();
            let ctx = ToolContext::new(
                "bench",
                "pdf.extract_images",
                paths,
                Limits::local(),
                &mut sink,
            );
            let before = Instant::now();
            let raw = fs::read(root.join(path)).unwrap();
            let old = ariad_pdf::document::open(&raw, ctx.limits());
            baseline += before.elapsed();
            drop((raw, old));
            let before = Instant::now();
            let result = crate::text_layout::read_and_preflight_pdf(&ctx);
            load += before.elapsed();
            match result {
                Ok((bytes, mut doc)) => {
                    let before = Instant::now();
                    match canonicalize(&ctx, &mut doc, bytes.len()) {
                        Ok(_) => accepted += 1,
                        Err(err) => eprintln!("BENCH REFUSED {path}: {}", err.message),
                    }
                    write += before.elapsed();
                }
                Err(err) => eprintln!("BENCH REFUSED {path}: {}", err.message),
            }
        }
        eprintln!(
            "TIMING count={count} accepted={accepted} baseline={baseline:?} load={load:?} canonical={write:?} combined={:?}",
            load + write
        );
    }
}
