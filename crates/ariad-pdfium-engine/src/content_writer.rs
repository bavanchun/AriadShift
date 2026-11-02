//! Canonical serialization of PDF content operands and operators.
use std::io::Write;

use ariad_engine_io::EngineFailure;
use lopdf::{Object, content::Content};

use crate::extract_images::next_content_token;

fn safe_name(token: &[u8]) -> bool {
    token.len() > 1
        && token.len() <= 255
        && token[0] == b'/'
        && token[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
}

fn scalar(token: &[u8]) -> bool {
    token.starts_with(b"/")
        || token.starts_with(b"(")
        || (token.starts_with(b"<") && !token.starts_with(b"<<"))
        || matches!(token, b"true" | b"false" | b"null")
        || token
            .first()
            .is_some_and(|b| b.is_ascii_digit() || b"+-.".contains(b))
}

fn write_operand(
    out: &mut Vec<u8>,
    bytes: &[u8],
    property_array: bool,
) -> Result<(), EngineFailure> {
    let mut source = bytes.to_vec();
    source.extend_from_slice(b" q");
    let content = Content::decode(&source).map_err(|_| EngineFailure::limit_exceeded())?;
    if content.operations.len() != 1 || content.operations[0].operands.len() != 1 {
        return Err(EngineFailure::limit_exceeded());
    }
    let object = &content.operations[0].operands[0];
    write_content_object_inner(out, object, property_array)
}

pub(crate) fn write_content_object(
    out: &mut Vec<u8>,
    object: &Object,
) -> Result<(), EngineFailure> {
    write_content_object_inner(out, object, false)
}

fn write_content_object_inner(
    out: &mut Vec<u8>,
    object: &Object,
    property_array: bool,
) -> Result<(), EngineFailure> {
    if property_array {
        let Object::Dictionary(dict) = object else {
            return Err(EngineFailure::limit_exceeded());
        };
        for (key, value) in dict.iter() {
            if !safe_name(&[b"/".as_slice(), key].concat()) {
                return Err(EngineFailure::limit_exceeded());
            }
            if let Object::Array(items) = value {
                for item in items {
                    if !matches!(
                        item,
                        Object::Name(_)
                            | Object::Integer(_)
                            | Object::Real(_)
                            | Object::String(_, _)
                    ) {
                        return Err(EngineFailure::limit_exceeded());
                    }
                    validate_object(item, 2)?;
                }
            } else {
                validate_object(value, 1)?;
            }
        }
    } else {
        validate_object(object, 0)?;
    }
    fn write_value(out: &mut Vec<u8>, object: &Object) -> Result<(), EngineFailure> {
        match object {
            Object::Array(items) => {
                out.extend_from_slice(b"[ ");
                for item in items {
                    write_value(out, item)?;
                    out.push(b' ');
                }
                out.push(b']');
            }
            Object::Dictionary(dict) => {
                out.extend_from_slice(b"<< ");
                for (key, value) in dict.iter() {
                    out.push(b'/');
                    out.extend_from_slice(key);
                    out.push(b' ');
                    write_value(out, value)?;
                    out.push(b' ');
                }
                out.extend_from_slice(b">>");
            }
            _ => crate::canonical::write_object(out, object)
                .map_err(|_| EngineFailure::limit_exceeded())?,
        }
        Ok(())
    }
    write_value(out, object)
}

pub(crate) fn validate_object(object: &Object, depth: usize) -> Result<(), EngineFailure> {
    match object {
        Object::Name(name) if safe_name(&[b"/".as_slice(), name].concat()) => Ok(()),
        Object::String(_, _) | Object::Null | Object::Boolean(_) => Ok(()),
        Object::Integer(value) if i32::try_from(*value).is_ok() => Ok(()),
        Object::Real(value) if value.is_finite() => Ok(()),
        Object::Array(items) if depth == 0 => {
            for item in items {
                validate_object(item, depth + 1)?;
            }
            Ok(())
        }
        Object::Dictionary(dict) if depth == 0 => {
            for (key, value) in dict.iter() {
                if !safe_name(&[b"/".as_slice(), key].concat()) {
                    return Err(EngineFailure::limit_exceeded());
                }
                validate_object(value, depth + 1)?;
            }
            Ok(())
        }
        _ => Err(EngineFailure::limit_exceeded()),
    }
}

/// Emits a non-inline token or an entire simple compound operand.
pub(crate) fn emit_token(
    bytes: &[u8],
    start: usize,
    end: usize,
    cursor: &mut usize,
    out: &mut Vec<u8>,
) -> Result<(), EngineFailure> {
    let token = &bytes[start..end];
    if token == b"[" || token == b"<<" {
        let is_dict = token == b"<<";
        let mut key = is_dict;
        let mut keys = std::collections::HashSet::new();
        let mut property_array = false;
        loop {
            let before = *cursor;
            let (s, e) =
                next_content_token(bytes, cursor)?.ok_or_else(EngineFailure::limit_exceeded)?;
            validate_gap(&bytes[before..s])?;
            let part = &bytes[s..e];
            if part == if is_dict { b">>".as_slice() } else { b"]" } {
                if is_dict && !key {
                    return Err(EngineFailure::limit_exceeded());
                }
                break;
            }
            if is_dict && !key && part == b"[" {
                property_array = true;
                loop {
                    let before = *cursor;
                    let (a, b) = next_content_token(bytes, cursor)?
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                    validate_gap(&bytes[before..a])?;
                    let item = &bytes[a..b];
                    if item == b"]" {
                        break;
                    }
                    if !scalar(item) || matches!(item, b"true" | b"false" | b"null") {
                        return Err(EngineFailure::limit_exceeded());
                    }
                    validate_token(item)?;
                }
                key = true;
                continue;
            }
            if !scalar(part) || (is_dict && key && (!safe_name(part) || !keys.insert(part))) {
                return Err(EngineFailure::limit_exceeded());
            }
            validate_token(part)?;
            if is_dict {
                key = !key;
            }
        }
        if property_array {
            let mut lookahead = *cursor;
            let (s, e) = next_content_token(bytes, &mut lookahead)?
                .ok_or_else(EngineFailure::limit_exceeded)?;
            validate_gap(&bytes[*cursor..s])?;
            if !matches!(&bytes[s..e], b"BDC" | b"DP") {
                return Err(EngineFailure::limit_exceeded());
            }
        }
        write_operand(out, &bytes[start..*cursor], property_array)?;
    } else if scalar(token) {
        validate_token(token)?;
        write_operand(out, token, false)?;
    } else {
        validate_token(token)?;
        if token.is_empty()
            || !token
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"*'\"".contains(b))
        {
            return Err(EngineFailure::limit_exceeded());
        }
        out.write_all(token)
            .map_err(|_| EngineFailure::limit_exceeded())?;
    }
    out.push(b' ');
    Ok(())
}

pub(crate) fn validate_token(token: &[u8]) -> Result<(), EngineFailure> {
    if token.starts_with(b"/") && !safe_name(token) {
        return Err(EngineFailure::limit_exceeded());
    }
    if !token.starts_with(b"(")
        && !token.starts_with(b"<")
        && token.iter().any(|b| !(0x21..=0x7e).contains(b))
    {
        return Err(EngineFailure::limit_exceeded());
    }
    Ok(())
}

pub(crate) fn validate_gap(bytes: &[u8]) -> Result<(), EngineFailure> {
    if bytes
        .iter()
        .any(|b| !(0x20..=0x7e).contains(b) && !matches!(*b, 0 | 9 | 10 | 12 | 13))
    {
        return Err(EngineFailure::limit_exceeded());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_arrays_only_allow_scalar_values_before_marked_content() {
        for input in [
            b"<< /Attached [/Top 1 (x) <79>] >> BDC".as_slice(),
            b"<< /Attached [/Top] >> DP",
        ] {
            let mut cursor = 0;
            let (start, end) = next_content_token(input, &mut cursor).unwrap().unwrap();
            let mut output = Vec::new();
            emit_token(input, start, end, &mut cursor, &mut output).unwrap();
            assert!(
                output.windows(4).any(|w| w == b"<78>") || output.windows(4).any(|w| w == b"/Top")
            );
        }
        for input in [
            b"<< /K [[/A]] >> BDC".as_slice(),
            b"<< /K [<< /A 1 >>] >> BDC",
            b"<< /K << /A 1 >> >> BDC",
            b"<< /K [true] >> BDC",
            b"<< /K [/A] >> q",
            b"[[/A]] TJ",
        ] {
            let mut cursor = 0;
            let (start, end) = next_content_token(input, &mut cursor).unwrap().unwrap();
            assert!(emit_token(input, start, end, &mut cursor, &mut Vec::new()).is_err());
        }
    }

    #[test]
    fn writes_simple_operands_and_rejects_ambiguous_structure() {
        for input in [b"[[ ]]".as_slice(), b"<< 1 2 >>", b"/Bad#20Name", b"q\xff"] {
            let mut cursor = 0;
            let (s, e) = next_content_token(input, &mut cursor).unwrap().unwrap();
            assert!(emit_token(input, s, e, &mut cursor, &mut Vec::new()).is_err());
        }
        let input = b"(hello)";
        let mut cursor = 0;
        let (s, e) = next_content_token(input, &mut cursor).unwrap().unwrap();
        let mut out = Vec::new();
        emit_token(input, s, e, &mut cursor, &mut out).unwrap();
        assert_eq!(out, b"<68656C6C6F> ");
    }
}
