use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use ariad_core::{
    ir::{Asset, AssetRef, AssetStore, Block, Document, Inline},
    limits::Limits,
    warning::{Warning, WarningCode},
};
use cap_std::{ambient_authority, fs::Dir};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("failed to open media directory: {0}")]
    MediaDirectory(#[source] io::Error),
}

/// Ingests media files extracted into `<work_dir>/media` by Pandoc.
///
/// For each image/figure referenced in the document:
/// - Confines the path lexically to `<work_dir>/media`.
/// - Never follows symlinks outside or inside the directory.
/// - Enforces `max_asset_bytes`.
/// - Sniffs media type from magic bytes (PNG, JPEG, GIF, WebP).
/// - Hashes the bytes with SHA-256 and inserts into `document.assets`.
/// - Rewrites the reference to `AssetRef::Asset { id }`.
///
/// Unreferenced media files are ignored.
/// Missing or invalid media emits a `WarningCode::ImageNotEmbedded` warning and
/// the image falls back to its alt text (or figure to its caption paragraph).
pub fn ingest_media(
    document: &mut Document,
    work_dir: &Path,
    limits: &Limits,
) -> Result<Vec<Warning>, MediaError> {
    let media_dir = work_dir.join("media");
    let mut warnings = Vec::new();

    let dir = match fs::canonicalize(&media_dir) {
        Ok(canonical) => Dir::open_ambient_dir(&canonical, ambient_authority()).ok(),
        Err(_) => None,
    };

    resolve_blocks(
        &mut document.body,
        &media_dir,
        dir.as_ref(),
        limits.max_asset_bytes,
        &mut document.assets,
        &mut warnings,
    );

    resolve_blocks(
        &mut document.furniture,
        &media_dir,
        dir.as_ref(),
        limits.max_asset_bytes,
        &mut document.assets,
        &mut warnings,
    );

    Ok(warnings)
}

fn extract_relative_path(href: &str, media_dir: &Path) -> Result<PathBuf, &'static str> {
    if href.is_empty() || href.contains('\0') {
        return Err("empty or null byte in path");
    }
    if href.contains(':') {
        return Err("path contains scheme or drive prefix");
    }
    if href.contains('\\') {
        return Err("path contains backslash");
    }

    let p = Path::new(href);
    let rel = if p.is_absolute() {
        if let Ok(rel) = p.strip_prefix(media_dir) {
            rel
        } else {
            return Err("absolute path escapes media directory");
        }
    } else if let Ok(rel) = p.strip_prefix("media") {
        rel
    } else if let Ok(rel) = p.strip_prefix("./media") {
        rel
    } else {
        p
    };

    for component in rel.components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err("path contains non-normal component or traverses directories"),
        }
    }

    Ok(rel.to_path_buf())
}

fn read_media(
    dir: &Dir,
    rel_path: &Path,
    max_asset_bytes: Option<u64>,
) -> Result<Asset, &'static str> {
    let metadata = match dir.symlink_metadata(rel_path) {
        Ok(m) => m,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Err("file not found"),
        Err(_) => return Err("I/O error reading metadata"),
    };

    if !metadata.file_type().is_file() {
        return Err("media path is not a regular file or is a symlink");
    }

    if max_asset_bytes.is_some_and(|max| metadata.len() > max) {
        return Err("media file exceeds max_asset_bytes");
    }

    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    let mut file = dir
        .open_with(rel_path, &options)
        .map_err(|_| "failed to open media file")?;

    let file_meta = file
        .metadata()
        .map_err(|_| "failed to query file metadata")?;
    if !file_meta.is_file() {
        return Err("media path is not a regular file");
    }

    let mut bytes = Vec::new();
    if let Some(max) = max_asset_bytes {
        let mut limited = (&mut file).take(max.saturating_add(1));
        limited
            .read_to_end(&mut bytes)
            .map_err(|_| "I/O error reading media bytes")?;
        if bytes.len() as u64 > max {
            return Err("media file exceeds max_asset_bytes");
        }
    } else {
        file.read_to_end(&mut bytes)
            .map_err(|_| "I/O error reading media bytes")?;
    }

    let media_type = sniff_image_type(&bytes).ok_or("unsupported media format")?;
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

fn resolve_media_asset(
    target: &AssetRef,
    media_dir: &Path,
    dir: Option<&Dir>,
    max_asset_bytes: Option<u64>,
    assets: &mut AssetStore,
) -> Result<String, &'static str> {
    match target {
        AssetRef::Asset { id } => Ok(id.clone()),
        AssetRef::Url { href } => {
            let rel_path = extract_relative_path(href, media_dir)?;
            let dir = dir.ok_or("media directory does not exist")?;
            let asset = read_media(dir, &rel_path, max_asset_bytes)?;
            let id = hex::encode(Sha256::digest(&asset.bytes));
            assets.entry(id.clone()).or_insert(asset);
            Ok(id)
        }
    }
}

fn resolve_inlines(
    inlines: &mut Vec<Inline>,
    media_dir: &Path,
    dir: Option<&Dir>,
    max_asset_bytes: Option<u64>,
    assets: &mut AssetStore,
    warnings: &mut Vec<Warning>,
) {
    let mut new_inlines = Vec::with_capacity(inlines.len());
    for inline in inlines.drain(..) {
        match inline {
            Inline::Image { target, alt, title } => {
                match resolve_media_asset(&target, media_dir, dir, max_asset_bytes, assets) {
                    Ok(id) => {
                        new_inlines.push(Inline::Image {
                            target: AssetRef::Asset { id },
                            alt,
                            title,
                        });
                    }
                    Err(reason) => {
                        warnings.push(Warning::new(
                            WarningCode::ImageNotEmbedded,
                            format!(
                                "media could not be embedded: {reason}; falling back to alt text"
                            ),
                        ));
                        if !alt.is_empty() {
                            new_inlines.push(Inline::Text { text: alt });
                        }
                    }
                }
            }
            Inline::Emph { mut content } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_inlines.push(Inline::Emph { content });
            }
            Inline::Strong { mut content } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_inlines.push(Inline::Strong { content });
            }
            Inline::Strikeout { mut content } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_inlines.push(Inline::Strikeout { content });
            }
            Inline::Superscript { mut content } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_inlines.push(Inline::Superscript { content });
            }
            Inline::Subscript { mut content } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_inlines.push(Inline::Subscript { content });
            }
            Inline::Link {
                url,
                title,
                mut content,
            } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_inlines.push(Inline::Link {
                    url,
                    title,
                    content,
                });
            }
            other => new_inlines.push(other),
        }
    }
    *inlines = new_inlines;
}

fn resolve_blocks(
    blocks: &mut Vec<Block>,
    media_dir: &Path,
    dir: Option<&Dir>,
    max_asset_bytes: Option<u64>,
    assets: &mut AssetStore,
    warnings: &mut Vec<Warning>,
) {
    let mut new_blocks = Vec::with_capacity(blocks.len());
    for block in blocks.drain(..) {
        match block {
            Block::Figure { asset, mut caption } => {
                resolve_inlines(
                    &mut caption,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                match resolve_media_asset(&asset, media_dir, dir, max_asset_bytes, assets) {
                    Ok(id) => {
                        new_blocks.push(Block::Figure {
                            asset: AssetRef::Asset { id },
                            caption,
                        });
                    }
                    Err(reason) => {
                        warnings.push(Warning::new(
                            WarningCode::ImageNotEmbedded,
                            format!(
                                "figure asset could not be embedded: {reason}; falling back to caption"
                            ),
                        ));
                        new_blocks.push(Block::Paragraph { content: caption });
                    }
                }
            }
            Block::Heading { level, mut content } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_blocks.push(Block::Heading { level, content });
            }
            Block::Paragraph { mut content } => {
                resolve_inlines(
                    &mut content,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_blocks.push(Block::Paragraph { content });
            }
            Block::Quote { mut blocks } => {
                resolve_blocks(
                    &mut blocks,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_blocks.push(Block::Quote { blocks });
            }
            Block::Footnote { id, mut blocks } => {
                resolve_blocks(
                    &mut blocks,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_blocks.push(Block::Footnote { id, blocks });
            }
            Block::List {
                ordered,
                start,
                tight,
                mut items,
            } => {
                for item in &mut items {
                    resolve_blocks(
                        &mut item.blocks,
                        media_dir,
                        dir,
                        max_asset_bytes,
                        assets,
                        warnings,
                    );
                }
                new_blocks.push(Block::List {
                    ordered,
                    start,
                    tight,
                    items,
                });
            }
            Block::Table {
                caption,
                columns,
                mut head,
                mut body,
                mut footnotes,
            } => {
                let mut new_caption = caption;
                if let Some(ref mut cap) = new_caption {
                    resolve_inlines(cap, media_dir, dir, max_asset_bytes, assets, warnings);
                }
                for row in head.iter_mut().chain(&mut body) {
                    for cell in row {
                        resolve_blocks(
                            &mut cell.blocks,
                            media_dir,
                            dir,
                            max_asset_bytes,
                            assets,
                            warnings,
                        );
                    }
                }
                resolve_inlines(
                    &mut footnotes,
                    media_dir,
                    dir,
                    max_asset_bytes,
                    assets,
                    warnings,
                );
                new_blocks.push(Block::Table {
                    caption: new_caption,
                    columns,
                    head,
                    body,
                    footnotes,
                });
            }
            other => new_blocks.push(other),
        }
    }
    *blocks = new_blocks;
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ariad_core::{
        ir::{AssetRef, Block, Document, Inline},
        limits::Limits,
        warning::WarningCode,
    };
    use tempfile::tempdir;

    use super::ingest_media;

    const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15c4\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82";

    #[test]
    fn valid_image_ingested_hashed_and_referenced() {
        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(media_dir.join("image1.png"), PNG_BYTES).unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Url {
                    href: format!("{}/image1.png", media_dir.display()),
                },
                alt: "Test image".to_owned(),
                title: None,
            }],
        });

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local()).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(doc.assets.len(), 1);

        let (id, asset) = doc.assets.iter().next().unwrap();
        assert_eq!(asset.media_type, "image/png");
        assert_eq!(asset.bytes, PNG_BYTES);

        match &doc.body[0] {
            Block::Paragraph { content } => match &content[0] {
                Inline::Image { target, alt, .. } => {
                    assert_eq!(target, &AssetRef::Asset { id: id.clone() });
                    assert_eq!(alt, "Test image");
                }
                other => panic!("expected Inline::Image, got {other:?}"),
            },
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn missing_image_warns_and_falls_back_to_alt_text() {
        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Url {
                    href: format!("{}/missing.png", media_dir.display()),
                },
                alt: "Fallback alt text".to_owned(),
                title: None,
            }],
        });

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local()).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, WarningCode::ImageNotEmbedded);
        assert!(doc.assets.is_empty());

        match &doc.body[0] {
            Block::Paragraph { content } => match &content[0] {
                Inline::Text { text } => {
                    assert_eq!(text, "Fallback alt text");
                }
                other => panic!("expected Inline::Text fallback, got {other:?}"),
            },
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn path_traversal_rejected_and_falls_back_to_alt_text() {
        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Url {
                    href: "media/../secret.txt".to_owned(),
                },
                alt: "Secret".to_owned(),
                title: None,
            }],
        });

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local()).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, WarningCode::ImageNotEmbedded);
        assert!(doc.assets.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_rejected_and_never_followed() {
        use std::os::unix::fs::symlink;

        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();

        let outside_file = work_dir.path().join("outside.png");
        fs::write(&outside_file, PNG_BYTES).unwrap();

        let symlink_path = media_dir.join("symlink.png");
        symlink(&outside_file, &symlink_path).unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Url {
                    href: format!("{}/symlink.png", media_dir.display()),
                },
                alt: "Symlink image".to_owned(),
                title: None,
            }],
        });

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local()).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, WarningCode::ImageNotEmbedded);
        assert!(doc.assets.is_empty());
    }

    #[test]
    fn max_asset_bytes_exceeded_rejected() {
        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(media_dir.join("large.png"), PNG_BYTES).unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Url {
                    href: format!("{}/large.png", media_dir.display()),
                },
                alt: "Large image".to_owned(),
                title: None,
            }],
        });

        let mut limits = Limits::local();
        limits.max_asset_bytes = Some(10); // smaller than PNG_BYTES

        let warnings = ingest_media(&mut doc, work_dir.path(), &limits).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, WarningCode::ImageNotEmbedded);
        assert!(doc.assets.is_empty());
    }

    #[test]
    fn unsupported_format_rejected() {
        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(media_dir.join("not_image.png"), b"plain text content").unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Url {
                    href: format!("{}/not_image.png", media_dir.display()),
                },
                alt: "Not image".to_owned(),
                title: None,
            }],
        });

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local()).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, WarningCode::ImageNotEmbedded);
        assert!(doc.assets.is_empty());
    }

    #[test]
    fn unreferenced_media_is_ignored() {
        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(media_dir.join("unreferenced.png"), PNG_BYTES).unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Text {
                text: "No images".to_owned(),
            }],
        });

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local()).unwrap();
        assert!(warnings.is_empty());
        assert!(doc.assets.is_empty());
    }
}
