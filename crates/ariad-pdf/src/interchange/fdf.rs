//! Bounded FDF parser and writer for AcroForm data.

use super::MAX_DATA_STRING_BYTES;

use std::collections::{BTreeMap, HashMap, HashSet};

use ariad_core::tools::params::form_model::{FormValue, MAX_FIELD_DEPTH, MAX_FIELDS};
use lopdf::{Object, StringFormat};

use super::{FormRecord, MAX_DATA_BYTES, check_data_size};
use crate::{PdfError, ToolCtx};

const MAX_TOKENS: usize = 1_000_000;
const MAX_DEPTH: usize = 64;
const MAX_STRING: usize = 1024 * 1024;

fn invalid() -> PdfError {
    PdfError::InvalidParams("invalid FDF input".into())
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    DictStart,
    DictEnd,
    ArrayStart,
    ArrayEnd,
    Name(Vec<u8>),
    String(Vec<u8>),
    Word(Vec<u8>),
}
#[derive(Debug)]
enum Node {
    Dict(BTreeMap<Vec<u8>, Node>),
    Array(Vec<Node>),
    String(Vec<u8>),
    Name(Vec<u8>),
    Ref(u32, u16),
    Bool(bool),
    Null,
    Other,
}

fn whitespace(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}
fn delimiter(b: u8) -> bool {
    whitespace(b) || b"()<>[]{}/%".contains(&b)
}
fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn tokenize(bytes: &[u8], ctx: &ToolCtx, block_limit: Option<u32>) -> Result<Vec<Token>, PdfError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if out.len() >= MAX_TOKENS {
            return Err(invalid());
        }
        if out.len() % 256 == 0 {
            ctx.check()?;
        }
        let c = bytes[i];
        if whitespace(c) {
            i += 1;
            continue;
        }
        if c == b'%' {
            while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                i += 1;
            }
            continue;
        }
        if let Some(limit) = block_limit
            && out.len() >= limit as usize
        {
            return Err(PdfError::LimitExceeded(
                ariad_core::reader::pdf::ReadError::TooManyBlocks { limit },
            ));
        }
        let token = match c {
            b'<' if bytes.get(i + 1) == Some(&b'<') => {
                i += 2;
                Token::DictStart
            }
            b'>' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                Token::DictEnd
            }
            b'[' => {
                i += 1;
                Token::ArrayStart
            }
            b']' => {
                i += 1;
                Token::ArrayEnd
            }
            b'/' => {
                i += 1;
                let mut data = Vec::new();
                while i < bytes.len() && !delimiter(bytes[i]) {
                    if bytes[i] == b'#' {
                        let a = bytes.get(i + 1).and_then(|b| hex(*b)).ok_or_else(invalid)?;
                        let b = bytes.get(i + 2).and_then(|b| hex(*b)).ok_or_else(invalid)?;
                        data.push(a * 16 + b);
                        i += 3;
                    } else {
                        data.push(bytes[i]);
                        i += 1;
                    }
                    if data.len() > MAX_STRING {
                        return Err(invalid());
                    }
                }
                Token::Name(data)
            }
            b'(' => {
                i += 1;
                let mut data = Vec::new();
                let mut nesting = 1usize;
                while nesting > 0 {
                    let b = *bytes.get(i).ok_or_else(invalid)?;
                    i += 1;
                    match b {
                        b'(' => {
                            nesting += 1;
                            if nesting > MAX_DEPTH {
                                return Err(invalid());
                            }
                            data.push(b);
                        }
                        b')' => {
                            nesting -= 1;
                            if nesting > 0 {
                                data.push(b);
                            }
                        }
                        b'\\' => {
                            let next = *bytes.get(i).ok_or_else(invalid)?;
                            i += 1;
                            match next {
                                b'n' => data.push(b'\n'),
                                b'r' => data.push(b'\r'),
                                b't' => data.push(b'\t'),
                                b'b' => data.push(8),
                                b'f' => data.push(12),
                                b'\n' => {}
                                b'\r' => {
                                    if bytes.get(i) == Some(&b'\n') {
                                        i += 1;
                                    }
                                }
                                b'0'..=b'7' => {
                                    let mut value = u16::from(next - b'0');
                                    for _ in 0..2 {
                                        if let Some(b'0'..=b'7') = bytes.get(i) {
                                            value = value * 8 + u16::from(bytes[i] - b'0');
                                            i += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                    data.push((value & 255) as u8);
                                }
                                other => data.push(other),
                            }
                        }
                        other => data.push(other),
                    }
                    if data.len() > MAX_STRING {
                        return Err(invalid());
                    }
                }
                Token::String(data)
            }
            b'<' => {
                i += 1;
                let mut data = Vec::new();
                let mut high = None;
                loop {
                    let b = *bytes.get(i).ok_or_else(invalid)?;
                    i += 1;
                    if b == b'>' {
                        break;
                    }
                    if whitespace(b) {
                        continue;
                    }
                    let v = hex(b).ok_or_else(invalid)?;
                    if let Some(h) = high.take() {
                        data.push(h * 16 + v);
                    } else {
                        high = Some(v);
                    }
                    if data.len() > MAX_STRING {
                        return Err(invalid());
                    }
                }
                if let Some(h) = high {
                    data.push(h * 16);
                }
                Token::String(data)
            }
            _ if !delimiter(c) => {
                let start = i;
                while i < bytes.len() && !delimiter(bytes[i]) {
                    i += 1;
                }
                if i - start > MAX_STRING {
                    return Err(invalid());
                }
                Token::Word(bytes[start..i].to_vec())
            }
            _ => return Err(invalid()),
        };
        out.push(token);
    }
    Ok(out)
}
fn number<T: std::str::FromStr>(t: Option<&Token>) -> Option<T> {
    let Token::Word(w) = t? else {
        return None;
    };
    std::str::from_utf8(w).ok()?.parse().ok()
}
fn word(t: Option<&Token>, expected: &[u8]) -> bool {
    matches!(t, Some(Token::Word(w)) if w == expected)
}
fn parse(tokens: &[Token], pos: &mut usize, depth: usize) -> Result<Node, PdfError> {
    if depth > MAX_DEPTH {
        return Err(invalid());
    }
    let t = tokens.get(*pos).ok_or_else(invalid)?;
    *pos += 1;
    Ok(match t {
        Token::DictStart => {
            let mut map = BTreeMap::new();
            while !matches!(tokens.get(*pos), Some(Token::DictEnd)) {
                let Some(Token::Name(key)) = tokens.get(*pos) else {
                    return Err(invalid());
                };
                *pos += 1;
                map.insert(key.clone(), parse(tokens, pos, depth + 1)?);
            }
            *pos += 1;
            Node::Dict(map)
        }
        Token::ArrayStart => {
            let mut array = Vec::new();
            while !matches!(tokens.get(*pos), Some(Token::ArrayEnd)) {
                array.push(parse(tokens, pos, depth + 1)?);
            }
            *pos += 1;
            Node::Array(array)
        }
        Token::String(s) => Node::String(s.clone()),
        Token::Name(s) => Node::Name(s.clone()),
        Token::Word(_) => {
            if let (Some(object), Some(generation), true) = (
                number(Some(t)),
                number(tokens.get(*pos)),
                word(tokens.get(*pos + 1), b"R"),
            ) {
                *pos += 2;
                Node::Ref(object, generation)
            } else if word(Some(t), b"true") {
                Node::Bool(true)
            } else if word(Some(t), b"false") {
                Node::Bool(false)
            } else if word(Some(t), b"null") {
                Node::Null
            } else {
                Node::Other
            }
        }
        _ => return Err(invalid()),
    })
}
fn resolve<'a>(
    mut node: &'a Node,
    objects: &'a HashMap<(u32, u16), Node>,
) -> Result<&'a Node, PdfError> {
    let mut seen = HashSet::new();
    while let Node::Ref(n, g) = node {
        if !seen.insert((*n, *g)) || seen.len() > MAX_DEPTH {
            return Err(invalid());
        }
        node = objects.get(&(*n, *g)).ok_or_else(invalid)?;
    }
    Ok(node)
}
fn dict<'a>(
    node: &'a Node,
    objects: &'a HashMap<(u32, u16), Node>,
) -> Result<&'a BTreeMap<Vec<u8>, Node>, PdfError> {
    match resolve(node, objects)? {
        Node::Dict(d) => Ok(d),
        _ => Err(invalid()),
    }
}
fn text(bytes: &[u8]) -> Result<String, PdfError> {
    let decoded = lopdf::decode_text_string(&Object::String(bytes.to_vec(), StringFormat::Literal))
        .map_err(|_| invalid())?;
    if decoded.len() > MAX_DATA_STRING_BYTES {
        return Err(invalid());
    }
    Ok(decoded)
}
fn value(node: &Node, objects: &HashMap<(u32, u16), Node>) -> Result<FormValue, PdfError> {
    match resolve(node, objects)? {
        Node::String(s) => Ok(FormValue::Text(text(s)?)),
        Node::Name(s) => Ok(FormValue::Text(
            std::str::from_utf8(s)
                .map(str::to_owned)
                .or_else(|_| text(s))?,
        )),
        Node::Array(a) => {
            let mut v = Vec::new();
            for item in a {
                match resolve(item, objects)? {
                    Node::String(s) | Node::Name(s) => v.push(text(s)?),
                    _ => return Err(invalid()),
                }
            }
            Ok(FormValue::Multi(v))
        }
        Node::Null => Ok(FormValue::Null(())),
        Node::Bool(b) => Ok(FormValue::Bool(*b)),
        _ => Err(invalid()),
    }
}
struct WalkState {
    seen: HashSet<(u32, u16)>,
    output: FormRecord,
    visited: usize,
}

fn walk(
    node: &Node,
    prefix: &str,
    objects: &HashMap<(u32, u16), Node>,
    state: &mut WalkState,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    state.visited = state.visited.saturating_add(1);
    if depth > MAX_FIELD_DEPTH || state.visited > MAX_FIELDS {
        return Err(invalid());
    }
    let key = if let Node::Ref(n, g) = node {
        if !state.seen.insert((*n, *g)) {
            return Err(invalid());
        }
        Some((*n, *g))
    } else {
        None
    };
    let field = dict(node, objects)?;
    let part = match field.get(b"T".as_slice()) {
        Some(Node::String(s)) => text(s)?,
        None => String::new(),
        _ => return Err(invalid()),
    };
    let name = if prefix.is_empty() {
        part
    } else if part.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}.{part}")
    };
    if name.len() > MAX_DATA_STRING_BYTES {
        return Err(invalid());
    }
    if let Some(v) = field.get(b"V".as_slice())
        && !name.is_empty()
    {
        state.output.insert(name.clone(), value(v, objects)?);
    }
    if let Some(kids) = field.get(b"Kids".as_slice()) {
        let Node::Array(kids) = resolve(kids, objects)? else {
            return Err(invalid());
        };
        for kid in kids {
            walk(kid, &name, objects, state, depth + 1, ctx)?;
        }
    }
    if let Some(key) = key {
        state.seen.remove(&key);
    }
    Ok(())
}

#[cfg(feature = "codecs")]
pub(super) fn check_limits(
    bytes: &[u8],
    limits: &ariad_core::limits::Limits,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let tokens = tokenize(bytes, ctx, Some(limits.max_blocks))?;
    let mut depth = 0usize;
    for token in tokens {
        match token {
            Token::DictStart | Token::ArrayStart => {
                depth += 1;
                if depth > limits.max_nesting_depth as usize {
                    return Err(PdfError::LimitExceeded(
                        ariad_core::reader::pdf::ReadError::NestingTooDeep {
                            limit: limits.max_nesting_depth,
                        },
                    ));
                }
            }
            Token::DictEnd | Token::ArrayEnd => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Read FDF field values without resolving file specifications or executing actions.
pub fn read_forms(bytes: &[u8], ctx: &ToolCtx) -> Result<FormRecord, PdfError> {
    check_data_size(bytes)?;
    ctx.check()?;
    if !bytes.starts_with(b"%FDF-") || !bytes.trim_ascii_end().ends_with(b"%%EOF") {
        return Err(invalid());
    }
    let tokens = tokenize(bytes, ctx, None)?;
    let mut objects = HashMap::new();
    let mut trailer = None;
    let mut pos = 0;
    while pos < tokens.len() {
        if word(tokens.get(pos), b"trailer") {
            pos += 1;
            trailer = Some(parse(&tokens, &mut pos, 0)?);
            break;
        }
        let Some(id) = number(tokens.get(pos)) else {
            return Err(invalid());
        };
        let generation = number(tokens.get(pos + 1)).ok_or_else(invalid)?;
        if !word(tokens.get(pos + 2), b"obj") {
            return Err(invalid());
        }
        pos += 3;
        let node = parse(&tokens, &mut pos, 0)?;
        if !word(tokens.get(pos), b"endobj") || objects.insert((id, generation), node).is_some() {
            return Err(invalid());
        }
        pos += 1;
    }
    let trailer = trailer.ok_or_else(invalid)?;
    if pos != tokens.len() {
        return Err(invalid());
    }
    let root = dict(&trailer, &objects)?
        .get(b"Root".as_slice())
        .ok_or_else(invalid)?;
    let fdf = dict(root, &objects)?
        .get(b"FDF".as_slice())
        .ok_or_else(invalid)?;
    let fields = dict(fdf, &objects)?
        .get(b"Fields".as_slice())
        .ok_or_else(invalid)?;
    let Node::Array(fields) = resolve(fields, &objects)? else {
        return Err(invalid());
    };
    let mut state = WalkState {
        seen: HashSet::new(),
        output: FormRecord::new(),
        visited: 0,
    };
    for field in fields {
        walk(field, "", &objects, &mut state, 0, ctx)?;
    }
    Ok(state.output)
}

#[derive(Default)]
struct FieldTree {
    value: Option<FormValue>,
    as_name: bool,
    children: BTreeMap<String, FieldTree>,
}
fn pdf_text(text: &str, output: &mut String) {
    use std::fmt::Write;
    output.push('<');
    for byte in crate::pdf_text::encode_bytes(text) {
        let _ = write!(output, "{byte:02X}");
    }
    output.push('>');
}
fn pdf_name(text: &str, output: &mut String) {
    use std::fmt::Write;
    output.push('/');
    for byte in text.bytes() {
        if (33..=126).contains(&byte) && !b"()<>[]{}/%#".contains(&byte) {
            output.push(byte as char);
        } else {
            let _ = write!(output, "#{byte:02X}");
        }
    }
}

fn emit_fields(
    fields: &BTreeMap<String, FieldTree>,
    output: &mut String,
    depth: usize,
) -> Result<(), PdfError> {
    if depth > MAX_FIELD_DEPTH {
        return Err(invalid());
    }
    output.push('[');
    for (name, node) in fields {
        output.push_str("<< /T ");
        pdf_text(name, output);
        if let Some(value) = &node.value {
            output.push_str(" /V ");
            match value {
                FormValue::Text(s) if node.as_name => pdf_name(s, output),
                FormValue::Text(s) => pdf_text(s, output),
                FormValue::Bool(true) => output.push_str("/Yes"),
                FormValue::Bool(false) => output.push_str("/Off"),
                FormValue::Null(()) => output.push_str("null"),
                FormValue::Multi(values) => {
                    output.push('[');
                    for value in values {
                        pdf_text(value, output);
                        output.push(' ');
                    }
                    output.push(']');
                }
            }
        }
        if !node.children.is_empty() {
            output.push_str(" /Kids ");
            emit_fields(&node.children, output, depth + 1)?;
        }
        output.push_str(" >>\n");
    }
    output.push(']');
    Ok(())
}

/// Encode one record as a deterministic FDF with hierarchical field names.
pub fn write_forms(record: &FormRecord) -> Result<Vec<u8>, PdfError> {
    write_forms_with_names(record, &HashSet::new())
}

/// Encode selected field values as PDF names for checkbox and radio states.
pub fn write_forms_with_names(
    record: &FormRecord,
    names: &HashSet<String>,
) -> Result<Vec<u8>, PdfError> {
    if record.len() > MAX_FIELDS {
        return Err(invalid());
    }
    let mut root = FieldTree::default();
    let mut budget = 128usize;
    for (name, value) in record {
        if name.is_empty() || name.len() > MAX_DATA_STRING_BYTES {
            return Err(invalid());
        }
        let value_bytes = match value {
            FormValue::Text(s) => s.len(),
            FormValue::Multi(values) => values
                .iter()
                .fold(0usize, |n, s| n.saturating_add(s.len()).saturating_add(8)),
            FormValue::Bool(_) | FormValue::Null(()) => 8,
        };
        if value_bytes > MAX_DATA_STRING_BYTES {
            return Err(invalid());
        }
        budget = budget
            .saturating_add(name.len().saturating_mul(8))
            .saturating_add(value_bytes.saturating_mul(4))
            .saturating_add(128);
        if budget > MAX_DATA_BYTES {
            return Err(invalid());
        }
        let mut node = &mut root;
        for (depth, part) in name.split('.').enumerate() {
            if part.is_empty() || depth >= MAX_FIELD_DEPTH {
                return Err(invalid());
            }
            node = node.children.entry(part.to_owned()).or_default();
        }
        node.value = Some(value.clone());
        node.as_name = names.contains(name);
    }
    let mut output = String::from("%FDF-1.2\n1 0 obj\n<< /FDF << /Fields ");
    emit_fields(&root.children, &mut output, 0)?;
    output.push_str(" >> >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
    check_data_size(output.as_bytes())?;
    Ok(output.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_hierarchy_and_unicode() {
        let record = FormRecord::from([
            ("person.name".into(), FormValue::Text("Tiếng Việt".into())),
            (
                "person.choices".into(),
                FormValue::Multi(vec!["a".into(), "b".into()]),
            ),
        ]);
        let encoded = write_forms(&record).unwrap();
        assert_eq!(read_forms(&encoded, &ToolCtx::new()).unwrap(), record);
    }
    #[test]
    fn rejects_truncated_and_cyclic_input() {
        assert!(read_forms(b"%FDF-1.2\n", &ToolCtx::new()).is_err());
        let cycle = b"%FDF-1.2\n1 0 obj << /FDF << /Fields [2 0 R] >> >> endobj\n2 0 obj << /T (x) /Kids [2 0 R] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        assert!(read_forms(cycle, &ToolCtx::new()).is_err());
    }
}
