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

/// How image references beneath an asset base are opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AssetPolicy {
    /// Ordinary local conversion: symlinks that stay inside the base are followed.
    Standard,
    /// Confined conversion: hidden components and symlinks are refused.
    Confined,
}

/// An already-open image base directory and the rule for reads beneath it.
///
/// Holding the handle, rather than a path, keeps image reads tied to the directory that was
/// authorized even if its path is later renamed or replaced.
#[derive(Debug)]
pub struct AssetBase {
    directory: Dir,
    policy: AssetPolicy,
}

impl AssetBase {
    /// Uses `directory` as a confined image base.
    ///
    /// References with a hidden component (a segment starting with `.`) are refused, and no
    /// path component may be a symlink, matching the confinement rules for the main input.
    #[must_use]
    pub fn confined(directory: Dir) -> Self {
        Self {
            directory,
            policy: AssetPolicy::Confined,
        }
    }
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
    let base = AssetBase {
        directory,
        policy: AssetPolicy::Standard,
    };
    Ok(resolve_in(document, &base, limits))
}

/// Embeds safe, local image references relative to an already-open asset base.
///
/// Unlike [`resolve`], this never performs a path lookup outside `base`.
pub fn resolve_in(document: &mut Document, base: &AssetBase, limits: &Limits) -> Vec<Warning> {
    let mut warnings = Vec::new();
    resolve_blocks(
        &mut document.body,
        base,
        limits.max_asset_bytes,
        &mut document.assets,
        &mut warnings,
    );
    warnings
}

fn resolve_blocks(
    blocks: &mut [Block],
    base: &AssetBase,
    max_asset_bytes: Option<u64>,
    assets: &mut ariad_core::ir::AssetStore,
    warnings: &mut Vec<Warning>,
) {
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                resolve_inlines(content, base, max_asset_bytes, assets, warnings);
            }
            Block::Quote { blocks } | Block::Footnote { blocks, .. } => {
                resolve_blocks(blocks, base, max_asset_bytes, assets, warnings);
            }
            Block::List { items, .. } => {
                for item in items {
                    resolve_blocks(&mut item.blocks, base, max_asset_bytes, assets, warnings);
                }
            }
            Block::Table {
                caption,
                head,
                body,
                ..
            } => {
                if let Some(caption) = caption {
                    resolve_inlines(caption, base, max_asset_bytes, assets, warnings);
                }
                for row in head.iter_mut().chain(body) {
                    for cell in row {
                        resolve_blocks(&mut cell.blocks, base, max_asset_bytes, assets, warnings);
                    }
                }
            }
            Block::Figure { asset, caption } => {
                resolve_reference(asset, base, max_asset_bytes, assets, warnings);
                resolve_inlines(caption, base, max_asset_bytes, assets, warnings);
            }
            Block::Code { .. } | Block::Math { .. } | Block::PageBreak {} | Block::Raw { .. } => {}
        }
    }
}

fn resolve_inlines(
    inlines: &mut [Inline],
    base: &AssetBase,
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
                resolve_inlines(content, base, max_asset_bytes, assets, warnings);
            }
            Inline::Image { target, .. } => {
                resolve_reference(target, base, max_asset_bytes, assets, warnings);
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
    base: &AssetBase,
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
    if base.policy == AssetPolicy::Confined && has_hidden_component(&path) {
        warn_not_embedded(warnings, "href contains a hidden path component");
        return;
    }

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
        let read = match base.policy {
            AssetPolicy::Standard => read_image(&base.directory, &candidate, max_asset_bytes),
            AssetPolicy::Confined => {
                read_image_nofollow(&base.directory, &candidate, max_asset_bytes)
            }
        };
        match read {
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

pub(crate) fn percent_decode(href: &str) -> Result<String, &'static str> {
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

pub(crate) fn lexical_path(decoded: &str) -> Result<PathBuf, &'static str> {
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

fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(component, Component::Normal(name) if name.to_string_lossy().starts_with('.'))
    })
}

pub(crate) fn has_url_scheme(value: &str) -> bool {
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
        .trim_end_matches([' ', '.']);
    let stem = stem.to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|number| matches!(number.as_bytes(), [b'1'..=b'9']))
        })
}

#[derive(Debug)]
pub(crate) enum ReadImageError {
    Missing,
    NotRegularFile,
    TooLarge,
    NotImage,
    Io,
}

pub(crate) fn read_image(
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
    let file = directory
        .open_with(path, &options)
        .map_err(|_| ReadImageError::Io)?;
    read_open_image(file, max_asset_bytes)
}

/// Reads an image beneath `directory` without following a symlink in any component.
///
/// Directory components are opened one at a time from the previous handle, and the file is
/// rechecked after opening, so a concurrent swap to a symlink is refused rather than followed.
fn read_image_nofollow(
    directory: &Dir,
    path: &Path,
    max_asset_bytes: Option<u64>,
) -> Result<Asset, ReadImageError> {
    let file_name = path.file_name().ok_or(ReadImageError::Missing)?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let parent = crate::confine::open_dir_nofollow(directory, parent).map_err(open_error)?;

    let entry = parent.symlink_metadata(file_name).map_err(open_error)?;
    if !entry.is_file() {
        return Err(ReadImageError::NotRegularFile);
    }
    let file = crate::confine::open_entry_nofollow(&parent, Path::new(file_name), false)
        .map_err(open_error)?;
    let opened = file.metadata().map_err(|_| ReadImageError::Io)?;
    let entry = parent.symlink_metadata(file_name).map_err(open_error)?;
    if crate::confine::is_reparse_point(&entry) || !entry.is_file() || !opened.is_file() {
        return Err(ReadImageError::NotRegularFile);
    }
    #[cfg(unix)]
    if !crate::confine::same_inode(&entry, &opened) {
        return Err(ReadImageError::NotRegularFile);
    }
    read_open_image(file, max_asset_bytes)
}

fn open_error(error: io::Error) -> ReadImageError {
    if error.kind() == io::ErrorKind::NotFound {
        ReadImageError::Missing
    } else {
        ReadImageError::Io
    }
}

fn read_open_image(
    mut file: cap_std::fs::File,
    max_asset_bytes: Option<u64>,
) -> Result<Asset, ReadImageError> {
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

pub(crate) fn sniff_image_type(bytes: &[u8]) -> Option<String> {
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

#[cfg(all(test, any(unix, windows)))]
mod nofollow_tests {
    use super::*;

    #[test]
    fn a_file_link_swapped_back_after_open_never_reads_hidden_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let hidden = temp.path().join(".private.png");
        let visible = temp.path().join("visible.png");
        let saved = temp.path().join("saved.png");
        fs::write(&hidden, b"\x89PNG\r\n\x1a\nprivate").unwrap();
        fs::write(&visible, b"\x89PNG\r\n\x1a\npublic").unwrap();
        let restore_visible = visible.clone();
        let restore_saved = saved.clone();
        crate::confine::TEST_OPEN_HOOKS.with(|hooks| {
            hooks.borrow_mut().extend([
                (
                    "file-before",
                    Box::new(move || {
                        fs::rename(&visible, saved).unwrap();
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(hidden, visible).unwrap();
                        #[cfg(windows)]
                        std::os::windows::fs::symlink_file(hidden, visible).unwrap();
                    }) as Box<dyn FnOnce()>,
                ),
                (
                    "file-after",
                    Box::new(move || {
                        fs::remove_file(&restore_visible).unwrap();
                        fs::rename(restore_saved, restore_visible).unwrap();
                    }) as Box<dyn FnOnce()>,
                ),
            ])
        });
        let read = read_image_nofollow(&root, Path::new("visible.png"), None);
        crate::confine::TEST_OPEN_HOOKS.with(|hooks| hooks.borrow_mut().clear());
        assert!(
            read.is_err(),
            "a swapped-in link to hidden bytes was accepted"
        );
    }
}
