//! Canonical serialization of PDF content operands and operators.
use std::io::Write;

use ariad_core::protocol::ErrorCode;
use ariad_engine_io::EngineFailure;
use lopdf::{Object, content::Content};

use crate::extract_images::next_content_token;

pub(crate) const OPERAND_MALFORMED: &str = "Content operand is malformed.";
pub(crate) const OPERAND_UNTERMINATED: &str = "Content operand is unterminated.";
pub(crate) const OPERAND_NESTED: &str = "Content operand nests unsupported containers.";
pub(crate) const DICTIONARY_KEY_INVALID: &str = "Content dictionary key is invalid or repeated.";
pub(crate) const DICTIONARY_VALUE_MISSING: &str = "Content dictionary key has no value.";
pub(crate) const PROPERTY_LIST_UNSUPPORTED: &str =
    "Marked-content property list has an unsupported value.";
pub(crate) const NAME_UNSUPPORTED: &str = "Content name is not supported.";
pub(crate) const NUMBER_UNSUPPORTED: &str = "Content number is out of range.";
pub(crate) const NON_ASCII: &str = "Content stream contains bytes outside printable ASCII.";
pub(crate) const OPERATOR_MALFORMED: &str = "Content operator is malformed.";

const fn refusal(message: &'static str) -> EngineFailure {
    EngineFailure::new(ErrorCode::LimitExceeded, message)
}

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
    let content = Content::decode(&source).map_err(|_| refusal(OPERAND_MALFORMED))?;
    if content.operations.len() != 1 || content.operations[0].operands.len() != 1 {
        return Err(refusal(OPERAND_MALFORMED));
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
            return Err(refusal(PROPERTY_LIST_UNSUPPORTED));
        };
        for (key, value) in dict.iter() {
            if !safe_name(&[b"/".as_slice(), key].concat()) {
                return Err(refusal(DICTIONARY_KEY_INVALID));
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
                        return Err(refusal(PROPERTY_LIST_UNSUPPORTED));
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
                    return Err(refusal(DICTIONARY_KEY_INVALID));
                }
                validate_object(value, depth + 1)?;
            }
            Ok(())
        }
        Object::Name(_) => Err(refusal(NAME_UNSUPPORTED)),
        Object::Integer(_) | Object::Real(_) => Err(refusal(NUMBER_UNSUPPORTED)),
        Object::Array(_) | Object::Dictionary(_) => Err(refusal(OPERAND_NESTED)),
        _ => Err(refusal(OPERAND_MALFORMED)),
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
                next_content_token(bytes, cursor)?.ok_or_else(|| refusal(OPERAND_UNTERMINATED))?;
            validate_gap(&bytes[before..s])?;
            let part = &bytes[s..e];
            if part == if is_dict { b">>".as_slice() } else { b"]" } {
                if is_dict && !key {
                    return Err(refusal(DICTIONARY_VALUE_MISSING));
                }
                break;
            }
            if is_dict && !key && part == b"[" {
                property_array = true;
                loop {
                    let before = *cursor;
                    let (a, b) = next_content_token(bytes, cursor)?
                        .ok_or_else(|| refusal(OPERAND_UNTERMINATED))?;
                    validate_gap(&bytes[before..a])?;
                    let item = &bytes[a..b];
                    if item == b"]" {
                        break;
                    }
                    if !scalar(item) || matches!(item, b"true" | b"false" | b"null") {
                        return Err(refusal(PROPERTY_LIST_UNSUPPORTED));
                    }
                    validate_token(item)?;
                }
                key = true;
                continue;
            }
            if !scalar(part) {
                return Err(refusal(OPERAND_NESTED));
            }
            if is_dict && key && (!safe_name(part) || !keys.insert(part)) {
                return Err(refusal(DICTIONARY_KEY_INVALID));
            }
            validate_token(part)?;
            if is_dict {
                key = !key;
            }
        }
        if property_array {
            let mut lookahead = *cursor;
            let (s, e) = next_content_token(bytes, &mut lookahead)?
                .ok_or_else(|| refusal(PROPERTY_LIST_UNSUPPORTED))?;
            validate_gap(&bytes[*cursor..s])?;
            if !matches!(&bytes[s..e], b"BDC" | b"DP") {
                return Err(refusal(PROPERTY_LIST_UNSUPPORTED));
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
            return Err(refusal(OPERATOR_MALFORMED));
        }
        out.write_all(token)
            .map_err(|_| EngineFailure::limit_exceeded())?;
    }
    out.push(b' ');
    Ok(())
}

pub(crate) fn validate_token(token: &[u8]) -> Result<(), EngineFailure> {
    if !token.starts_with(b"(")
        && !token.starts_with(b"<")
        && token.iter().any(|b| !(0x21..=0x7e).contains(b))
    {
        return Err(refusal(NON_ASCII));
    }
    if token.starts_with(b"/") && !safe_name(token) {
        return Err(refusal(NAME_UNSUPPORTED));
    }
    Ok(())
}

pub(crate) fn validate_gap(bytes: &[u8]) -> Result<(), EngineFailure> {
    if bytes
        .iter()
        .any(|b| !(0x20..=0x7e).contains(b) && !matches!(*b, 0 | 9 | 10 | 12 | 13))
    {
        return Err(refusal(NON_ASCII));
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
        for (input, expected) in [
            (b"<< /K [[/A]] >> BDC".as_slice(), PROPERTY_LIST_UNSUPPORTED),
            (b"<< /K [<< /A 1 >>] >> BDC", PROPERTY_LIST_UNSUPPORTED),
            (b"<< /K << /A 1 >> >> BDC", OPERAND_NESTED),
            (b"<< /K [true] >> BDC", PROPERTY_LIST_UNSUPPORTED),
            (b"<< /K [/A] >> q", PROPERTY_LIST_UNSUPPORTED),
            (b"[[/A]] TJ", OPERAND_NESTED),
        ] {
            let mut cursor = 0;
            let (start, end) = next_content_token(input, &mut cursor).unwrap().unwrap();
            let err = emit_token(input, start, end, &mut cursor, &mut Vec::new()).unwrap_err();
            assert_eq!(err.code, ErrorCode::LimitExceeded);
            assert_eq!(err.message, expected, "{input:?}");
        }
    }

    #[test]
    fn writes_simple_operands_and_rejects_ambiguous_structure() {
        for (input, expected) in [
            (b"[[ ]]".as_slice(), OPERAND_NESTED),
            (b"<< 1 2 >>", DICTIONARY_KEY_INVALID),
            (b"<< /A >>", DICTIONARY_VALUE_MISSING),
            (b"[ /A", OPERAND_UNTERMINATED),
            (b"/Bad#20Name", NAME_UNSUPPORTED),
            (b"q\xff", NON_ASCII),
        ] {
            let mut cursor = 0;
            let (s, e) = next_content_token(input, &mut cursor).unwrap().unwrap();
            let err = emit_token(input, s, e, &mut cursor, &mut Vec::new()).unwrap_err();
            assert_eq!(err.code, ErrorCode::LimitExceeded);
            assert_eq!(err.message, expected, "{input:?}");
        }
        let err = validate_object(&Object::Real(f32::INFINITY), 0).unwrap_err();
        assert_eq!(err.message, NUMBER_UNSUPPORTED);
        let err = validate_object(&Object::Array(vec![Object::Array(vec![])]), 0).unwrap_err();
        assert_eq!(err.message, OPERAND_NESTED);
        let input = b"(hello)";
        let mut cursor = 0;
        let (s, e) = next_content_token(input, &mut cursor).unwrap().unwrap();
        let mut out = Vec::new();
        emit_token(input, s, e, &mut cursor, &mut out).unwrap();
        assert_eq!(out, b"<68656C6C6F> ");
    }
}
