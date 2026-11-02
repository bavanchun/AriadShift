use super::{ColorSpace, MappedColor, dict, resolve, space};
use lopdf::{Dictionary, Document, Object, StringFormat};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) enum Special {
    Indexed,
    Pattern(ColorSpace),
}

pub(super) fn convert<F>(
    doc: &Document,
    resources: &Dictionary,
    mapper: &F,
    limits: &ariad_core::limits::Limits,
    budget: &mut crate::document::DecodeBudget,
) -> Result<(Dictionary, BTreeMap<Vec<u8>, Special>), crate::PdfError>
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    let max_bytes = limits.max_decompressed_bytes;
    let Some(spaces) = resources
        .get(b"ColorSpace")
        .ok()
        .and_then(|o| dict(doc, o, limits.max_nesting_depth))
    else {
        return Ok((resources.clone(), BTreeMap::new()));
    };
    let mut result = spaces.clone();
    let mut special = BTreeMap::new();
    for (name, entry) in spaces {
        let Some(array) =
            resolve(doc, entry, limits.max_nesting_depth).and_then(|o| o.as_array().ok())
        else {
            continue;
        };
        let Some(kind) = array.first().and_then(|o| o.as_name().ok()) else {
            continue;
        };
        if matches!(kind, b"Indexed" | b"I") && array.len() == 4 {
            let Some(base_name) = array[1].as_name().ok() else {
                continue;
            };
            let Some(base) = space(doc, resources, base_name, limits.max_nesting_depth) else {
                continue;
            };
            let Some(high) = array[2]
                .as_i64()
                .ok()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n <= 255)
            else {
                continue;
            };
            let Some(lookup) = resolve(doc, &array[3], limits.max_nesting_depth) else {
                continue;
            };
            let needed = (high + 1) * base.count();
            if needed as u64 > max_bytes {
                continue;
            }
            let data = match lookup {
                Object::String(s, _) if s.len() >= needed => {
                    budget.charge(needed)?;
                    s[..needed].to_vec()
                }
                Object::Stream(s) => {
                    budget.charge(max_bytes.min(1024) as usize)?;
                    match s.get_plain_content_with_limit(max_bytes.min(1024) as usize) {
                        Ok(v) => v,
                        Err(_) => continue,
                    }
                }
                _ => continue,
            };
            if data.len() < needed {
                continue;
            }
            budget.charge((high + 1) * 4)?;
            let mut out = Vec::new();
            let mut target = None;
            for chunk in data[..needed].chunks_exact(base.count()) {
                let values = chunk
                    .iter()
                    .map(|x| f32::from(*x) / 255.0)
                    .collect::<Vec<_>>();
                let Some(mapped) = mapper(base, &values) else {
                    target = None;
                    break;
                };
                if mapped.components.len() != mapped.space.count()
                    || mapped.components.iter().any(|n| !n.is_finite())
                    || target.is_some_and(|t| t != mapped.space)
                {
                    target = None;
                    break;
                }
                target = Some(mapped.space);
                out.extend(
                    mapped
                        .components
                        .into_iter()
                        .map(|n| (n.clamp(0.0, 1.0) * 255.0).round() as u8),
                );
            }
            let Some(target) = target else { continue };
            if out.len() as u64 > max_bytes {
                continue;
            }
            result.set(
                name.clone(),
                Object::Array(vec![
                    Object::Name(b"Indexed".to_vec()),
                    Object::Name(target.name().to_vec()),
                    Object::Integer(high as i64),
                    Object::String(out, StringFormat::Hexadecimal),
                ]),
            );
            special.insert(name.clone(), Special::Indexed);
        } else if kind == b"Pattern" && array.len() == 2 {
            let Some(base_name) = array[1].as_name().ok() else {
                continue;
            };
            let Some(base) = space(doc, resources, base_name, limits.max_nesting_depth) else {
                continue;
            };
            let Some(mapped) = mapper(base, &vec![0.0; base.count()]) else {
                continue;
            };
            if mapped.components.len() != mapped.space.count()
                || mapped.components.iter().any(|v| !v.is_finite())
            {
                continue;
            }
            result.set(
                name.clone(),
                Object::Array(vec![
                    Object::Name(b"Pattern".to_vec()),
                    Object::Name(mapped.space.name().to_vec()),
                ]),
            );
            special.insert(name.clone(), Special::Pattern(base));
        }
    }
    let mut out = resources.clone();
    out.set("ColorSpace", result);
    Ok((out, special))
}
