//! Conservative GID-preserving TrueType outline pruning.

use std::collections::BTreeSet;

fn u16_at(data: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(pos..pos.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(data: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(pos..pos.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn put_u32(data: &mut [u8], pos: usize, value: u32) -> Option<()> {
    data.get_mut(pos..pos.checked_add(4)?)?
        .copy_from_slice(&value.to_be_bytes());
    Some(())
}
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0_u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

struct Table<'a> {
    tag: [u8; 4],
    data: &'a [u8],
}

/// Remove unused glyf outlines while retaining glyph IDs, cmap and metrics.
/// Returns None for unsupported or malformed fonts.
pub(super) fn subset(bytes: &[u8], used: &BTreeSet<u16>) -> Option<Vec<u8>> {
    if !matches!(bytes.get(..4)?, [0, 1, 0, 0] | b"true") {
        return None;
    }
    let count = usize::from(u16_at(bytes, 4)?);
    if count == 0 || count > 256 {
        return None;
    }
    let mut tables = Vec::with_capacity(count);
    for i in 0..count {
        let pos = 12_usize.checked_add(i.checked_mul(16)?)?;
        let tag: [u8; 4] = bytes.get(pos..pos + 4)?.try_into().ok()?;
        // Colour, bitmap and variable tables may reference additional glyph IDs.
        if matches!(
            &tag,
            b"COLR" | b"SVG " | b"sbix" | b"CBDT" | b"EBDT" | b"gvar"
        ) {
            return None;
        }
        let offset = usize::try_from(u32_at(bytes, pos + 8)?).ok()?;
        let len = usize::try_from(u32_at(bytes, pos + 12)?).ok()?;
        let data = bytes.get(offset..offset.checked_add(len)?)?;
        if tables.iter().any(|t: &Table<'_>| t.tag == tag) {
            return None;
        }
        tables.push(Table { tag, data });
    }
    let table = |tag: &[u8; 4]| tables.iter().find(|t| &t.tag == tag).map(|t| t.data);
    let head = table(b"head")?;
    let maxp = table(b"maxp")?;
    let loca = table(b"loca")?;
    let glyf = table(b"glyf")?;
    let glyph_count = usize::from(u16_at(maxp, 4)?);
    let long = match u16_at(head, 50)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let mut offsets = Vec::with_capacity(glyph_count + 1);
    for i in 0..=glyph_count {
        let off = if long {
            usize::try_from(u32_at(loca, i.checked_mul(4)?)?).ok()?
        } else {
            usize::from(u16_at(loca, i.checked_mul(2)?)?).checked_mul(2)?
        };
        if off > glyf.len() || offsets.last().is_some_and(|last| *last > off) {
            return None;
        }
        offsets.push(off);
    }
    let mut keep = used.clone();
    keep.insert(0);
    let mut queue: Vec<u16> = keep.iter().copied().collect();
    while let Some(id) = queue.pop() {
        let id = usize::from(id);
        if id >= glyph_count {
            return None;
        }
        let glyph = glyf.get(offsets[id]..offsets[id + 1])?;
        if glyph.is_empty() {
            continue;
        }
        let contours = i16::from_be_bytes(glyph.get(..2)?.try_into().ok()?);
        if contours >= 0 {
            continue;
        }
        let mut pos = 10_usize;
        loop {
            let flags = u16_at(glyph, pos)?;
            let component = u16_at(glyph, pos.checked_add(2)?)?;
            if usize::from(component) >= glyph_count {
                return None;
            }
            if keep.insert(component) {
                queue.push(component);
            }
            pos = pos.checked_add(4)?;
            pos = pos.checked_add(if flags & 1 != 0 { 4 } else { 2 })?;
            pos = pos.checked_add(if flags & 8 != 0 {
                2
            } else if flags & 64 != 0 {
                4
            } else if flags & 128 != 0 {
                8
            } else {
                0
            })?;
            if pos > glyph.len() {
                return None;
            }
            if flags & 32 == 0 {
                break;
            }
        }
    }
    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity((glyph_count + 1) * if long { 4 } else { 2 });
    for i in 0..=glyph_count {
        let offset = u32::try_from(new_glyf.len()).ok()?;
        if long {
            new_loca.extend_from_slice(&offset.to_be_bytes());
        } else {
            new_loca.extend_from_slice(&u16::try_from(offset / 2).ok()?.to_be_bytes());
        }
        if i < glyph_count && keep.contains(&u16::try_from(i).ok()?) {
            new_glyf.extend_from_slice(glyf.get(offsets[i]..offsets[i + 1])?);
            if !new_glyf.len().is_multiple_of(2) {
                new_glyf.push(0);
            }
        }
    }
    let header_len = 12_usize.checked_add(count.checked_mul(16)?)?;
    let mut output = bytes.get(..header_len)?.to_vec();
    let mut head_offset = None;
    for (i, entry) in tables.iter().enumerate() {
        let mut data = match &entry.tag {
            b"glyf" => new_glyf.clone(),
            b"loca" => new_loca.clone(),
            _ => entry.data.to_vec(),
        };
        if &entry.tag == b"head" {
            put_u32(&mut data, 8, 0)?;
            head_offset = Some(output.len());
        }
        let pos = 12 + i * 16;
        put_u32(&mut output, pos + 4, checksum(&data))?;
        let offset = u32::try_from(output.len()).ok()?;
        put_u32(&mut output, pos + 8, offset)?;
        put_u32(&mut output, pos + 12, u32::try_from(data.len()).ok()?)?;
        output.extend_from_slice(&data);
        while !output.len().is_multiple_of(4) {
            output.push(0);
        }
    }
    let adjustment = 0xB1B0AFBA_u32.wrapping_sub(checksum(&output));
    put_u32(&mut output, head_offset?.checked_add(8)?, adjustment)?;
    Some(output)
}

/// Collect all plausible cmap glyph IDs for a Unicode character and source code.
pub(super) fn glyphs_for_code(bytes: &[u8], ch: char, code: u8) -> Option<BTreeSet<u16>> {
    let count = usize::from(u16_at(bytes, 4)?);
    let mut cmap = None;
    for i in 0..count {
        let pos = 12_usize.checked_add(i.checked_mul(16)?)?;
        if bytes.get(pos..pos + 4)? == b"cmap" {
            let offset = usize::try_from(u32_at(bytes, pos + 8)?).ok()?;
            let length = usize::try_from(u32_at(bytes, pos + 12)?).ok()?;
            cmap = Some(bytes.get(offset..offset.checked_add(length)?)?);
        }
    }
    let cmap = cmap?;
    let count = usize::from(u16_at(cmap, 2)?);
    let mut result = BTreeSet::new();
    let mut seen = false;
    for i in 0..count {
        let pos = 4_usize.checked_add(i.checked_mul(8)?)?;
        let platform = u16_at(cmap, pos)?;
        let encoding = u16_at(cmap, pos + 2)?;
        let off = usize::try_from(u32_at(cmap, pos + 4)?).ok()?;
        let sub = cmap.get(off..)?;
        let charcode = match (platform, encoding) {
            (0, _) | (3, 1 | 10) => u32::from(ch),
            (1, 0) => u32::from(code),
            _ => continue,
        };
        let glyph = match u16_at(sub, 0)? {
            0 => {
                let len = usize::from(u16_at(sub, 2)?);
                let sub = sub.get(..len)?;
                if charcode < 256 {
                    u16::from(*sub.get(6 + charcode as usize)?)
                } else {
                    0
                }
            }
            4 => {
                let len = usize::from(u16_at(sub, 2)?);
                let sub = sub.get(..len)?;
                if charcode > 65535 {
                    continue;
                }
                let count = usize::from(u16_at(sub, 6)?) / 2;
                let ends = 14_usize;
                let starts = ends.checked_add(count.checked_mul(2)?)?.checked_add(2)?;
                let deltas = starts.checked_add(count.checked_mul(2)?)?;
                let ranges = deltas.checked_add(count.checked_mul(2)?)?;
                let mut mapped = 0;
                for j in 0..count {
                    let end = u32::from(u16_at(sub, ends + j * 2)?);
                    let start = u32::from(u16_at(sub, starts + j * 2)?);
                    if charcode < start || charcode > end {
                        continue;
                    }
                    let delta = u16_at(sub, deltas + j * 2)?;
                    let range = usize::from(u16_at(sub, ranges + j * 2)?);
                    mapped = if range == 0 {
                        (charcode as u16).wrapping_add(delta)
                    } else {
                        let idx = (ranges + j * 2)
                            .checked_add(range)?
                            .checked_add(usize::try_from(charcode - start).ok()?.checked_mul(2)?)?;
                        let raw = u16_at(sub, idx)?;
                        if raw == 0 { 0 } else { raw.wrapping_add(delta) }
                    };
                    break;
                }
                mapped
            }
            6 => {
                let len = usize::from(u16_at(sub, 2)?);
                let sub = sub.get(..len)?;
                let first = u32::from(u16_at(sub, 6)?);
                let entries = u32::from(u16_at(sub, 8)?);
                if charcode >= first && charcode - first < entries {
                    u16_at(
                        sub,
                        10 + usize::try_from(charcode - first).ok()?.checked_mul(2)?,
                    )?
                } else {
                    0
                }
            }
            12 => {
                let len = usize::try_from(u32_at(sub, 4)?).ok()?;
                let sub = sub.get(..len)?;
                let groups = usize::try_from(u32_at(sub, 12)?).ok()?;
                let mut mapped = 0;
                for j in 0..groups {
                    let pos = 16_usize.checked_add(j.checked_mul(12)?)?;
                    let start = u32_at(sub, pos)?;
                    let end = u32_at(sub, pos + 4)?;
                    if (start..=end).contains(&charcode) {
                        mapped =
                            u16::try_from(u32_at(sub, pos + 8)?.checked_add(charcode - start)?)
                                .ok()?;
                        break;
                    }
                }
                mapped
            }
            _ => continue,
        };
        seen = true;
        result.insert(glyph);
    }
    seen.then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_components_survive_outline_pruning() {
        let mut head = vec![0; 54];
        head[50..52].copy_from_slice(&1_u16.to_be_bytes());
        let mut maxp = vec![0; 6];
        maxp[4..6].copy_from_slice(&4_u16.to_be_bytes());
        let simple = [0_u8; 10];
        let mut compound = [0_u8; 16];
        compound[0..2].copy_from_slice(&(-1_i16).to_be_bytes());
        compound[12..14].copy_from_slice(&2_u16.to_be_bytes());
        let glyf = [
            simple.as_slice(),
            compound.as_slice(),
            simple.as_slice(),
            simple.as_slice(),
        ]
        .concat();
        let mut loca = Vec::new();
        for offset in [0_u32, 10, 26, 36, 46] {
            loca.extend_from_slice(&offset.to_be_bytes());
        }
        let tables: [(&[u8; 4], &[u8]); 4] = [
            (b"head", &head),
            (b"maxp", &maxp),
            (b"loca", &loca),
            (b"glyf", &glyf),
        ];
        let mut font = vec![0; 12 + 4 * 16];
        font[..4].copy_from_slice(&[0, 1, 0, 0]);
        font[4..6].copy_from_slice(&4_u16.to_be_bytes());
        for (i, (tag, data)) in tables.iter().enumerate() {
            let entry = 12 + i * 16;
            font[entry..entry + 4].copy_from_slice(*tag);
            let offset = font.len() as u32;
            font[entry + 8..entry + 12].copy_from_slice(&offset.to_be_bytes());
            font[entry + 12..entry + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
            font.extend_from_slice(data);
            while !font.len().is_multiple_of(4) {
                font.push(0);
            }
        }
        let subset = subset(&font, &BTreeSet::from([1])).unwrap();
        let loca_pos = u32_at(&subset, 12 + 2 * 16 + 8).unwrap() as usize;
        let offsets: Vec<_> = (0..=4)
            .map(|i| u32_at(&subset, loca_pos + i * 4).unwrap())
            .collect();
        assert_eq!(offsets, [0, 10, 26, 36, 36]);
        assert_eq!(checksum(&subset), 0xB1B0AFBA);
    }
}
