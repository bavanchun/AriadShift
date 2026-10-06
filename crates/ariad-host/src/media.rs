use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{
    ir::{AssetRef, AssetStore, Block, Document, Inline},
    limits::Limits,
    warning::{Warning, WarningCode},
};
use cap_std::{ambient_authority, fs::Dir};
use sha2::{Digest, Sha256};

use crate::assets;

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
pub fn ingest_media(document: &mut Document, work_dir: &Path, limits: &Limits) -> Vec<Warning> {
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

    warnings
}

fn is_windows_drive_path(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn extract_relative_path(href: &str, media_dir: &Path) -> Result<PathBuf, &'static str> {
    let decoded = assets::percent_decode(href).map_err(|_| "href has invalid percent encoding")?;
    if decoded.is_empty() || decoded.contains('\0') {
        return Err("empty or null byte in path");
    }
    if !is_windows_drive_path(&decoded) && assets::has_url_scheme(&decoded) {
        return Err("path contains URL scheme");
    }

    let forward_slash_href = decoded.replace('\\', "/");
    let media_dir_str = media_dir.to_string_lossy().replace('\\', "/");
    let media_dir_prefix = if media_dir_str.ends_with('/') {
        media_dir_str
    } else {
        format!("{media_dir_str}/")
    };

    let rel_str = if forward_slash_href
        .get(..media_dir_prefix.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(&media_dir_prefix))
    {
        forward_slash_href
            .get(media_dir_prefix.len()..)
            .unwrap_or("")
    } else if let Some(rest) = forward_slash_href.strip_prefix("./media/") {
        rest
    } else if let Some(rest) = forward_slash_href.strip_prefix("media/") {
        rest
    } else if forward_slash_href.starts_with('/') || forward_slash_href.contains(':') {
        return Err("absolute path escapes media directory");
    } else {
        &forward_slash_href
    };

    assets::lexical_path(rel_str)
        .map_err(|_| "path traverses directories or contains invalid components")
}

fn resolve_media_asset(
    target: &AssetRef,
    media_dir: &Path,
    dir: Option<&Dir>,
    max_asset_bytes: Option<u64>,
    assets_store: &mut AssetStore,
) -> Result<String, &'static str> {
    match target {
        AssetRef::Asset { id } => Ok(id.clone()),
        AssetRef::Url { href } => {
            let rel_path = extract_relative_path(href, media_dir)?;
            let dir = dir.ok_or("media directory does not exist")?;
            let asset =
                assets::read_image(dir, &rel_path, max_asset_bytes).map_err(|err| match err {
                    assets::ReadImageError::Missing => "media file not found",
                    assets::ReadImageError::NotRegularFile => {
                        "media path is not a regular file or is a symlink"
                    }
                    assets::ReadImageError::TooLarge => "media file exceeds max_asset_bytes",
                    assets::ReadImageError::NotImage => "unsupported media format",
                    assets::ReadImageError::Io => "I/O error reading media bytes",
                })?;
            let id = hex::encode(Sha256::digest(&asset.bytes));
            assets_store.entry(id.clone()).or_insert(asset);
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
    use std::{fs, path::Path};

    use ariad_core::{
        ir::{AssetRef, Block, Document, Inline},
        limits::Limits,
        warning::WarningCode,
    };
    use tempfile::tempdir;

    use super::{extract_relative_path, ingest_media};

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

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local());
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

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local());
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

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local());
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

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local());
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

        let warnings = ingest_media(&mut doc, work_dir.path(), &limits);
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

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local());
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

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local());
        assert!(warnings.is_empty());
        assert!(doc.assets.is_empty());
    }

    #[test]
    fn windows_style_absolute_media_path_confined_and_resolved() {
        let media_dir = Path::new(r"C:\work\tmp\media");
        let href = r"C:\work\tmp\media\sub\image.png";
        let rel = extract_relative_path(href, media_dir).unwrap();
        assert_eq!(rel, Path::new("sub/image.png"));
    }

    #[test]
    fn windows_drive_letter_case_insensitive() {
        let media_dir = Path::new(r"C:\work\tmp\media");
        let href = r"c:\work\tmp\media\image.png";
        let rel = extract_relative_path(href, media_dir).unwrap();
        assert_eq!(rel, Path::new("image.png"));
    }

    #[test]
    fn path_traversal_with_windows_separators_rejected() {
        let media_dir = Path::new("/work/tmp/media");
        assert!(extract_relative_path(r"media\..\..\secret.txt", media_dir).is_err());
        assert!(extract_relative_path(r"..\secret.txt", media_dir).is_err());
        assert!(extract_relative_path(r"C:\Windows\System32\cmd.exe", media_dir).is_err());
    }

    #[test]
    fn percent_encoded_media_path_resolved() {
        let work_dir = tempdir().unwrap();
        let media_dir = work_dir.path().join("media");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(media_dir.join("image with space.png"), PNG_BYTES).unwrap();

        let mut doc = Document::default();
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Url {
                    href: "media/image%20with%20space.png".to_owned(),
                },
                alt: "Space image".to_owned(),
                title: None,
            }],
        });

        let warnings = ingest_media(&mut doc, work_dir.path(), &Limits::local());
        assert!(
            warnings.is_empty(),
            "expected no warnings, got: {warnings:?}"
        );
        assert_eq!(doc.assets.len(), 1);
        let id = doc.assets.keys().next().unwrap();
        match &doc.body[0] {
            Block::Paragraph { content } => match &content[0] {
                Inline::Image { target, .. } => {
                    assert_eq!(target, &AssetRef::Asset { id: id.clone() });
                }
                other => panic!("expected Inline::Image, got {other:?}"),
            },
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn multibyte_vietnamese_image_href_never_panics_at_any_work_dir_length() {
        for prefix_len in 40..=80 {
            let base = "x".repeat(prefix_len);
            let work_dir = Path::new(&base);
            let href = "hình_ảnh_minh_họa_tiếng_việt_chất_lượng_cao_12345.png";
            let mut doc = Document::default();
            doc.body.push(Block::Paragraph {
                content: vec![Inline::Image {
                    target: AssetRef::Url {
                        href: href.to_owned(),
                    },
                    alt: "Hình minh họa".to_owned(),
                    title: None,
                }],
            });

            let warnings = ingest_media(&mut doc, work_dir, &Limits::local());
            assert_eq!(warnings.len(), 1, "failed at prefix_len {prefix_len}");
            assert_eq!(warnings[0].code, WarningCode::ImageNotEmbedded);
            match &doc.body[0] {
                Block::Paragraph { content } => match &content[0] {
                    Inline::Text { text } => {
                        assert_eq!(text, "Hình minh họa");
                    }
                    other => panic!("expected Inline::Text, got {other:?}"),
                },
                other => panic!("expected Paragraph, got {other:?}"),
            }
        }
    }

    #[test]
    fn probe_multibyte_repeated_chars_missing_image_at_variable_prefix_lengths() {
        for prefix_len in 40..=80 {
            let base = format!("/tmp/{}", "a".repeat(prefix_len - 5));
            let work_dir = Path::new(&base);
            let href = "ảảảảảảảảảảảảảảảảảảả.png";
            let mut doc = Document::default();
            doc.body.push(Block::Paragraph {
                content: vec![Inline::Image {
                    target: AssetRef::Url {
                        href: href.to_owned(),
                    },
                    alt: "Fallback alt".to_owned(),
                    title: None,
                }],
            });

            let warnings = ingest_media(&mut doc, work_dir, &Limits::local());
            assert_eq!(warnings.len(), 1, "failed at prefix_len {prefix_len}");
            assert_eq!(warnings[0].code, WarningCode::ImageNotEmbedded);
            match &doc.body[0] {
                Block::Paragraph { content } => match &content[0] {
                    Inline::Text { text } => {
                        assert_eq!(text, "Fallback alt");
                    }
                    other => panic!("expected Inline::Text, got {other:?}"),
                },
                other => panic!("expected Paragraph, got {other:?}"),
            }
        }
    }
}
