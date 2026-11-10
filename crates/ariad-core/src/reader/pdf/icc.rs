//! Recognize matrix/TRC sRGB profiles without treating arbitrary ICC RGB as sRGB.
//! Colorimetry follows https://registry.color.org/rgb-registry/srgb.
use std::collections::BTreeMap;

pub(super) const MAX_PROFILE_BYTES: usize = 64 * 1024;

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Recognizes the standard D50-adapted sRGB matrix and parametric transfer curves.
/// Names, dates and profile IDs are never evidence of colorimetry.
pub(super) fn is_srgb(data: &[u8]) -> bool {
    let Some(tags) = tags(data) else {
        return false;
    };
    let expected = [
        (*b"wtpt", vec![0xf6d6, 0x10000, 0xd32d]),
        (*b"rXYZ", vec![0x6fa0, 0x38f2, 0x038f]),
        (*b"gXYZ", vec![0x6296, 0xb789, 0x18da]),
        (*b"bXYZ", vec![0x24a0, 0x0f85, 0xb6c4]),
    ];
    for (tag, values) in expected {
        let Some(payload) = tags.get(&tag) else {
            return false;
        };
        if payload.len() != 20
            || &payload[..8] != b"XYZ \0\0\0\0"
            || values
                .iter()
                .enumerate()
                .any(|(i, value)| u32_at(payload, 8 + i * 4) != Some(*value))
        {
            return false;
        }
    }
    let curve = [0x26669, 0xf2a7, 0x0d59, 0x13d0, 0x0a5b];
    for tag in [*b"rTRC", *b"gTRC", *b"bTRC"] {
        let Some(payload) = tags.get(&tag) else {
            return false;
        };
        if payload.len() != 32
            || &payload[..12] != b"para\0\0\0\0\0\x03\0\0"
            || curve
                .iter()
                .enumerate()
                .any(|(i, value)| u32_at(payload, 12 + i * 4) != Some(*value))
        {
            return false;
        }
    }
    let adaptation = [
        0x10c3f, 0x05dd, 0xfffff326, 0x0790, 0xfd92, 0xfffffba1, 0xfffffda2, 0x03dc, 0xc071,
    ];
    let Some(payload) = tags.get(b"chad") else {
        return false;
    };
    payload.len() == 44
        && &payload[..8] == b"sf32\0\0\0\0"
        && adaptation
            .iter()
            .enumerate()
            .all(|(i, value)| u32_at(payload, 8 + i * 4) == Some(*value))
}

fn tags(data: &[u8]) -> Option<BTreeMap<[u8; 4], &[u8]>> {
    if data.len() < 132
        || data.len() > MAX_PROFILE_BYTES
        || u32_at(data, 0)? as usize != data.len()
        || &data[16..24] != b"RGB XYZ "
        || &data[36..40] != b"acsp"
        || !matches!(data[8], 2 | 4)
    {
        return None;
    }
    let count = u32_at(data, 128)? as usize;
    if count > 32 {
        return None;
    }
    let table_end = 132 + count * 12;
    if table_end > data.len() {
        return None;
    }
    let mut tags = BTreeMap::new();
    for i in 0..count {
        let entry = 132 + i * 12;
        let tag: [u8; 4] = data[entry..entry + 4].try_into().ok()?;
        // Refuse LUT transforms and other colorimetry overrides, regardless of the profile name.
        if !matches!(
            &tag,
            b"desc"
                | b"cprt"
                | b"wtpt"
                | b"chad"
                | b"rXYZ"
                | b"gXYZ"
                | b"bXYZ"
                | b"rTRC"
                | b"gTRC"
                | b"bTRC"
        ) {
            return None;
        }
        let offset = u32_at(data, entry + 4)? as usize;
        let length = u32_at(data, entry + 8)? as usize;
        if offset < table_end
            || !offset.is_multiple_of(4)
            || tags
                .insert(tag, data.get(offset..offset.checked_add(length)?)?)
                .is_some()
        {
            return None;
        }
    }
    Some(tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Vec<u8> {
        let mut payloads = Vec::new();
        for (tag, values) in [
            (*b"wtpt", vec![0xf6d6u32, 0x10000, 0xd32d]),
            (*b"rXYZ", vec![0x6fa0, 0x38f2, 0x038f]),
            (*b"gXYZ", vec![0x6296, 0xb789, 0x18da]),
            (*b"bXYZ", vec![0x24a0, 0x0f85, 0xb6c4]),
        ] {
            let mut bytes = b"XYZ \0\0\0\0".to_vec();
            bytes.extend(values.into_iter().flat_map(u32::to_be_bytes));
            payloads.push((tag, bytes));
        }
        for tag in [*b"rTRC", *b"gTRC", *b"bTRC"] {
            let mut bytes = b"para\0\0\0\0\0\x03\0\0".to_vec();
            bytes.extend(
                [0x26669u32, 0xf2a7, 0x0d59, 0x13d0, 0x0a5b]
                    .into_iter()
                    .flat_map(u32::to_be_bytes),
            );
            payloads.push((tag, bytes));
        }
        let mut bytes = b"sf32\0\0\0\0".to_vec();
        bytes.extend(
            [
                0x10c3fu32, 0x05dd, 0xfffff326, 0x0790, 0xfd92, 0xfffffba1, 0xfffffda2, 0x03dc,
                0xc071,
            ]
            .into_iter()
            .flat_map(u32::to_be_bytes),
        );
        payloads.push((*b"chad", bytes));
        let mut data = vec![0; 132 + payloads.len() * 12];
        data[8] = 4;
        data[16..24].copy_from_slice(b"RGB XYZ ");
        data[36..40].copy_from_slice(b"acsp");
        data[128..132].copy_from_slice(&(payloads.len() as u32).to_be_bytes());
        for (i, (tag, bytes)) in payloads.into_iter().enumerate() {
            let offset = data.len() as u32;
            let entry = 132 + i * 12;
            data[entry..entry + 4].copy_from_slice(&tag);
            data[entry + 4..entry + 8].copy_from_slice(&offset.to_be_bytes());
            data[entry + 8..entry + 12].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
            data.extend(bytes);
        }
        let length = data.len() as u32;
        data[..4].copy_from_slice(&length.to_be_bytes());
        data
    }
    #[test]
    fn recognition_requires_colorimetry_and_valid_bounded_offsets() {
        let data = profile();
        assert!(is_srgb(&data));
        for offset in [16, 36, 132, data.len() - 1] {
            let mut changed = data.clone();
            changed[offset] ^= 1;
            assert!(!is_srgb(&changed));
        }
        let mut changed = data.clone();
        changed[136..140].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(!is_srgb(&changed));
        assert!(!is_srgb(&data[..data.len() - 1]));
        let mut named = data;
        named[80..84].copy_from_slice(b"sRGB");
        let last = named.len() - 1;
        named[last] ^= 1;
        assert!(!is_srgb(&named));
    }
}
