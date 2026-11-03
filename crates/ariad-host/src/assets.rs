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
            AssetPolicy::Standard => read_image(&base.directory, &candidate, max_asset_bytes)
                .map_err(ReadNofollowError::Image),
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
            Err(ReadNofollowError::Image(ReadImageError::Missing)) => continue,
            Err(ReadNofollowError::Hidden) => {
                failure_reason = "href contains a hidden path component";
                break;
            }
            Err(ReadNofollowError::Image(ReadImageError::NotRegularFile)) => {
                failure_reason = "image path is not a regular file";
                break;
            }
            Err(ReadNofollowError::Image(ReadImageError::TooLarge)) => {
                failure_reason = "image exceeds max_asset_bytes";
                break;
            }
            Err(ReadNofollowError::Image(ReadImageError::NotImage)) => {
                failure_reason = "file does not contain a supported image format";
                break;
            }
            Err(ReadNofollowError::Image(ReadImageError::Io)) => break,
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

#[cfg(any(windows, test))]
const WINDOWS_FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

/// Returns `true` when `name` looks like a Windows 8.3 short-name alias
/// (a non-empty stem prefix followed by `~` and ASCII digits before the extension).
#[cfg(any(windows, test))]
fn looks_like_windows_short_name(name: &str) -> bool {
    let stem = name.split_once('.').map_or(name, |(stem, _)| stem);
    let rstem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    has_short_alias_tilde(stem) || has_short_alias_tilde(rstem)
}

#[cfg(any(windows, test))]
fn has_short_alias_tilde(stem: &str) -> bool {
    let Some((prefix, digits)) = stem.rsplit_once('~') else {
        return false;
    };
    !prefix.is_empty() && !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Pure Windows hidden-component decision for a single path component.
///
/// Rejects:
/// - a component whose requested or long name starts with `.`;
/// - an entry carrying `FILE_ATTRIBUTE_HIDDEN` (`0x2`);
/// - a component that looks like an 8.3 short alias (`~` followed by digits before the
///   extension), unless it matches `long_name` (case-insensitively).
#[cfg(any(windows, test))]
fn is_windows_hidden_component(
    requested_name: &str,
    long_name: Option<&str>,
    file_attributes: u32,
) -> bool {
    if requested_name.starts_with('.') || long_name.is_some_and(|actual| actual.starts_with('.')) {
        return true;
    }
    if file_attributes & WINDOWS_FILE_ATTRIBUTE_HIDDEN != 0 {
        return true;
    }
    if looks_like_windows_short_name(requested_name) {
        return !long_name.is_some_and(|actual| actual.eq_ignore_ascii_case(requested_name));
    }
    false
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

#[derive(Debug)]
enum ReadNofollowError {
    #[cfg_attr(not(windows), allow(dead_code))]
    Hidden,
    Image(ReadImageError),
}

impl From<ReadImageError> for ReadNofollowError {
    fn from(error: ReadImageError) -> Self {
        Self::Image(error)
    }
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
) -> Result<Asset, ReadNofollowError> {
    let file_name = path.file_name().ok_or(ReadImageError::Missing)?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let parent = open_confined_asset_dir(directory, parent)?;

    let entry = parent.symlink_metadata(file_name).map_err(open_error)?;
    if !entry.is_file() {
        return Err(ReadImageError::NotRegularFile.into());
    }
    let file = crate::confine::open_entry_nofollow(&parent, Path::new(file_name), false)
        .map_err(open_error)?;
    let opened = file.metadata().map_err(|_| ReadImageError::Io)?;
    let entry = parent.symlink_metadata(file_name).map_err(open_error)?;
    if crate::confine::is_reparse_point(&entry) || !entry.is_file() || !opened.is_file() {
        return Err(ReadImageError::NotRegularFile.into());
    }
    #[cfg(unix)]
    if !crate::confine::same_inode(&entry, &opened) {
        return Err(ReadImageError::NotRegularFile.into());
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        let long_name = resolve_windows_component_long_name(&parent, file_name)?;
        let attributes = entry.file_attributes() | opened.file_attributes();
        if is_windows_hidden_component(
            &file_name.to_string_lossy(),
            long_name.as_deref(),
            attributes,
        ) {
            return Err(ReadNofollowError::Hidden);
        }
    }
    Ok(read_open_image(file, max_asset_bytes)?)
}

fn open_confined_asset_dir(root_dir: &Dir, relative: &Path) -> Result<Dir, ReadNofollowError> {
    #[cfg(not(windows))]
    {
        Ok(crate::confine::open_dir_nofollow(root_dir, relative).map_err(open_error)?)
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        let mut current = root_dir.try_clone().map_err(|_| ReadImageError::Io)?;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(ReadImageError::Io.into());
            };
            let next =
                crate::confine::open_dir_nofollow(&current, Path::new(name)).map_err(open_error)?;
            let entry = current.symlink_metadata(name).map_err(open_error)?;
            let opened = next.dir_metadata().map_err(|_| ReadImageError::Io)?;
            let long_name = resolve_windows_component_long_name(&current, name)?;
            let attributes = entry.file_attributes() | opened.file_attributes();
            if is_windows_hidden_component(
                &name.to_string_lossy(),
                long_name.as_deref(),
                attributes,
            ) {
                return Err(ReadNofollowError::Hidden);
            }
            current = next;
        }
        Ok(current)
    }
}

#[cfg(windows)]
fn resolve_windows_component_long_name(
    parent: &Dir,
    name: &std::ffi::OsStr,
) -> Result<Option<String>, ReadImageError> {
    let requested = name.to_string_lossy();
    if !looks_like_windows_short_name(&requested) {
        return Ok(None);
    }
    for entry in parent.entries().map_err(|_| ReadImageError::Io)? {
        let entry = entry.map_err(|_| ReadImageError::Io)?;
        let file_name = entry.file_name();
        let actual = file_name.to_string_lossy();
        if actual.eq_ignore_ascii_case(&requested) {
            return Ok(Some(actual.into_owned()));
        }
    }
    Ok(None)
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

#[cfg(test)]
mod windows_hidden_tests {
    use super::*;

    #[test]
    fn windows_short_name_detection_identifies_tilde_digit_aliases() {
        for alias in [
            "PRIVAT~1",
            "PHOTO~1.PNG",
            "DOCUME~12.txt",
            "archive~1.tar.gz",
        ] {
            assert!(
                looks_like_windows_short_name(alias),
                "{alias} should look like a Windows 8.3 short alias"
            );
        }
        for normal in [
            "photo.png",
            "my~photo.png",
            "photo~.png",
            "~1.png",
            "photo.png~1",
        ] {
            assert!(
                !looks_like_windows_short_name(normal),
                "{normal} should not be treated as a Windows 8.3 short alias"
            );
        }
    }

    #[test]
    fn windows_hidden_component_rejects_hidden_attribute_and_unmatched_short_aliases() {
        const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
        const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

        // Files carrying FILE_ATTRIBUTE_HIDDEN are always rejected.
        assert!(is_windows_hidden_component(
            "photo.png",
            Some("photo.png"),
            WINDOWS_FILE_ATTRIBUTE_HIDDEN
        ));
        assert!(is_windows_hidden_component(
            "photo.png",
            None,
            FILE_ATTRIBUTE_ARCHIVE | WINDOWS_FILE_ATTRIBUTE_HIDDEN
        ));
        assert!(is_windows_hidden_component(
            "notes~1.png",
            Some("notes~1.png"),
            WINDOWS_FILE_ATTRIBUTE_HIDDEN
        ));

        // 8.3 short aliases that do not match the long name are rejected.
        assert!(is_windows_hidden_component("PRIVAT~1", Some(".private"), 0));
        assert!(is_windows_hidden_component("PRIVAT~1", None, 0));
        assert!(is_windows_hidden_component(
            "PHOTO~1.PNG",
            Some(".photo.png"),
            0
        ));
        assert!(is_windows_hidden_component(
            "PHOTO~1.PNG",
            Some("photograph.png"),
            0
        ));

        // A component that looks like an 8.3 alias is allowed when it matches the long name.
        assert!(!is_windows_hidden_component(
            "notes~1.png",
            Some("notes~1.png"),
            FILE_ATTRIBUTE_NORMAL
        ));
        assert!(!is_windows_hidden_component(
            "NOTES~1.PNG",
            Some("notes~1.png"),
            FILE_ATTRIBUTE_NORMAL
        ));

        // Ordinary visible names are allowed, while dot-prefixed names are rejected.
        assert!(!is_windows_hidden_component(
            "photo.png",
            None,
            FILE_ATTRIBUTE_NORMAL
        ));
        assert!(!is_windows_hidden_component(
            "my~photo.png",
            None,
            FILE_ATTRIBUTE_NORMAL
        ));
        assert!(is_windows_hidden_component(".photo.png", None, 0));
        assert!(is_windows_hidden_component(
            "photo.png",
            Some(".photo.png"),
            0
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_confined_read_rejects_hidden_attribute_and_allows_matching_tilde_name() {
        use std::os::windows::fs::OpenOptionsExt;

        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();

        let hidden_path = temp.path().join("hidden.png");
        let mut hidden_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .attributes(WINDOWS_FILE_ATTRIBUTE_HIDDEN)
            .open(&hidden_path)
            .unwrap();
        io::Write::write_all(&mut hidden_file, b"\x89PNG\r\n\x1a\nhidden").unwrap();
        drop(hidden_file);

        assert!(matches!(
            read_image_nofollow(&root, Path::new("hidden.png"), None),
            Err(ReadNofollowError::Hidden)
        ));

        let literal_tilde = temp.path().join("notes~1.png");
        fs::write(&literal_tilde, b"\x89PNG\r\n\x1a\nvisible").unwrap();
        assert!(read_image_nofollow(&root, Path::new("notes~1.png"), None).is_ok());
    }
}
