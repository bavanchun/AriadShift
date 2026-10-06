use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use ariad_core::{
    ir::{Asset, AssetRef, Block, Document, Inline},
    limits::Limits,
    warning::{Warning, WarningCode},
};
use cap_std::{ambient_authority, fs::Dir};
use sha2::{Digest, Sha256};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("image base directory could not be opened")]
    BaseDirectory(#[source] io::Error),
}

/// Embeds safe, local image references from the input document's directory.
///
/// External URLs and any path that fails lexical validation remain URLs and produce warnings.
/// Lexical validation happens before a href is used in a filesystem operation.
pub fn resolve(
    document: &mut Document,
    base_dir: &Path,
    limits: &Limits,
) -> Result<Vec<Warning>, AssetError> {
    let base_dir = if base_dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        base_dir
    };
    let canonical_base = fs::canonicalize(base_dir).map_err(AssetError::BaseDirectory)?;
    let directory = Dir::open_ambient_dir(canonical_base, ambient_authority())
        .map_err(AssetError::BaseDirectory)?;
    let mut warnings = Vec::new();
    resolve_blocks(
        &mut document.body,
        &directory,
        limits.max_asset_bytes,
        &mut document.assets,
        &mut warnings,
    );
    Ok(warnings)
}

fn resolve_blocks(
    blocks: &mut [Block],
    directory: &Dir,
    max_asset_bytes: Option<u64>,
    assets: &mut ariad_core::ir::AssetStore,
    warnings: &mut Vec<Warning>,
) {
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                resolve_inlines(content, directory, max_asset_bytes, assets, warnings);
            }
            Block::Quote { blocks } | Block::Footnote { blocks, .. } => {
                resolve_blocks(blocks, directory, max_asset_bytes, assets, warnings);
            }
            Block::List { items, .. } => {
                for item in items {
                    resolve_blocks(
                        &mut item.blocks,
                        directory,
                        max_asset_bytes,
                        assets,
                        warnings,
                    );
                }
            }
            Block::Table {
                caption,
                head,
                body,
                ..
            } => {
                if let Some(caption) = caption {
                    resolve_inlines(caption, directory, max_asset_bytes, assets, warnings);
                }
                for row in head.iter_mut().chain(body) {
                    for cell in row {
                        resolve_blocks(
                            &mut cell.blocks,
                            directory,
                            max_asset_bytes,
                            assets,
                            warnings,
                        );
                    }
                }
            }
            Block::Figure { asset, caption } => {
                resolve_reference(asset, directory, max_asset_bytes, assets, warnings);
                resolve_inlines(caption, directory, max_asset_bytes, assets, warnings);
            }
            Block::Code { .. } | Block::Math { .. } | Block::PageBreak {} | Block::Raw { .. } => {}
        }
    }
}

fn resolve_inlines(
    inlines: &mut [Inline],
    directory: &Dir,
    max_asset_bytes: Option<u64>,
    assets: &mut ariad_core::ir::AssetStore,
    warnings: &mut Vec<Warning>,
) {
    for inline in inlines {
        match inline {
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => {
                resolve_inlines(content, directory, max_asset_bytes, assets, warnings);
            }
            Inline::Image { target, .. } => {
                resolve_reference(target, directory, max_asset_bytes, assets, warnings);
            }
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::SoftBreak {}
            | Inline::LineBreak {}
            | Inline::Math { .. }
            | Inline::FootnoteRef { .. }
            | Inline::Raw { .. } => {}
        }
    }
}

fn resolve_reference(
    reference: &mut AssetRef,
    directory: &Dir,
    max_asset_bytes: Option<u64>,
    assets: &mut ariad_core::ir::AssetStore,
    warnings: &mut Vec<Warning>,
) {
    let AssetRef::Url { href } = reference else {
        return;
    };

    let decoded = match percent_decode(href) {
        Ok(decoded) => decoded,
        Err(reason) => {
            warn_not_embedded(warnings, reason);
            return;
        }
    };
    let path = match lexical_path(&decoded) {
        Ok(path) => path,
        Err(reason) => {
            warn_not_embedded(warnings, reason);
            return;
        }
    };

    let normalized: String = path
        .to_str()
        .expect("decoded href remains UTF-8")
        .nfc()
        .collect();
    let decomposed: String = normalized.nfd().collect();
    let candidates = if normalized == decomposed {
        vec![PathBuf::from(normalized)]
    } else {
        vec![PathBuf::from(normalized), PathBuf::from(decomposed)]
    };

    let mut failure_reason = "image file could not be read from the input directory";
    for candidate in candidates {
        match read_image(directory, &candidate, max_asset_bytes) {
            Ok(asset) => {
                let id = hex::encode(Sha256::digest(&asset.bytes));
                assets.entry(id.clone()).or_insert(asset);
                *reference = AssetRef::Asset { id };
                return;
            }
            Err(ReadImageError::Missing) => continue,
            Err(ReadImageError::NotRegularFile) => {
                failure_reason = "image path is not a regular file";
                break;
            }
            Err(ReadImageError::TooLarge) => {
                failure_reason = "image exceeds max_asset_bytes";
                break;
            }
            Err(ReadImageError::NotImage) => {
                failure_reason = "file does not contain a supported image format";
                break;
            }
            Err(ReadImageError::Io) => break,
        }
    }
    warn_not_embedded(warnings, failure_reason);
}

fn warn_not_embedded(warnings: &mut Vec<Warning>, reason: &'static str) {
    warnings.push(Warning::new(
        WarningCode::ImageNotEmbedded,
        format!("image was not embedded: {reason}"),
    ));
}

fn percent_decode(href: &str) -> Result<String, &'static str> {
    let bytes = href.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).and_then(|byte| hex_value(*byte));
            let low = bytes.get(index + 2).and_then(|byte| hex_value(*byte));
            let (Some(high), Some(low)) = (high, low) else {
                return Err("href has an invalid percent escape");
            };
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| "href is not valid UTF-8 after percent decoding")
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn lexical_path(decoded: &str) -> Result<PathBuf, &'static str> {
    if decoded.is_empty() || decoded.contains('\0') {
        return Err("href is empty or contains a null byte");
    }
    if has_url_scheme(decoded) {
        return Err("href contains a URL scheme");
    }
    if decoded.contains('\\') {
        return Err("href contains a backslash path separator");
    }
    if decoded.contains(':') {
        return Err("href contains a colon or drive prefix");
    }
    if decoded.starts_with('/') {
        return Err("href is an absolute or rooted path");
    }

    let segments: Vec<_> = decoded.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err("href contains an empty path segment");
    }
    if segments.contains(&"..") {
        return Err("href contains a parent directory segment");
    }
    if segments.contains(&".") {
        return Err("href contains a current directory segment");
    }
    #[cfg(windows)]
    if segments
        .iter()
        .any(|segment| is_windows_device_name(segment))
    {
        return Err("href contains a reserved Windows device name");
    }

    let path = Path::new(decoded);
    if !path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("href contains a non-relative path component");
    }
    Ok(path.to_path_buf())
}

fn has_url_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    let mut bytes = scheme.bytes();
    matches!(bytes.next(), Some(first) if first.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-'))
}

#[cfg(windows)]
fn is_windows_device_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(|character| character == ' ' || character == '.');
    let stem = stem.to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|number| matches!(number.as_bytes(), [b'1'..=b'9']))
        })
}

enum ReadImageError {
    Missing,
    NotRegularFile,
    TooLarge,
    NotImage,
    Io,
}

fn read_image(
    directory: &Dir,
    path: &Path,
    max_asset_bytes: Option<u64>,
) -> Result<Asset, ReadImageError> {
    let metadata = match directory.symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ReadImageError::Missing);
        }
        Err(_) => return Err(ReadImageError::Io),
    };
    if !metadata.file_type().is_file() {
        return Err(ReadImageError::NotRegularFile);
    }

    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    let mut file = directory
        .open_with(path, &options)
        .map_err(|_| ReadImageError::Io)?;
    let metadata = file.metadata().map_err(|_| ReadImageError::Io)?;
    if !metadata.is_file() {
        return Err(ReadImageError::NotRegularFile);
    }
    if max_asset_bytes.is_some_and(|maximum| metadata.len() > maximum) {
        return Err(ReadImageError::TooLarge);
    }

    let mut bytes = Vec::new();
    if let Some(maximum) = max_asset_bytes {
        let mut limited = file.take(maximum.saturating_add(1));
        limited
            .read_to_end(&mut bytes)
            .map_err(|_| ReadImageError::Io)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
            return Err(ReadImageError::TooLarge);
        }
    } else {
        file.read_to_end(&mut bytes)
            .map_err(|_| ReadImageError::Io)?;
    }

    let media_type = sniff_image_type(&bytes).ok_or(ReadImageError::NotImage)?;
    Ok(Asset { media_type, bytes })
}

fn sniff_image_type(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png".to_owned())
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg".to_owned())
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif".to_owned())
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp".to_owned())
    } else {
        None
    }
}
