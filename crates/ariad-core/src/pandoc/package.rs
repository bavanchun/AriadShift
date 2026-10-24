//! Pandoc interchange and embedded media for memory-only runtimes.
use crate::{
    format::Format,
    ir::{Asset, AssetRef, AssetStore, Block, Inline},
    json_depth::{json_depth_budget_for_nesting, prescan_json_depth},
    limits::Limits,
    pipeline::{self, Notice, PipelineError, Read as ReadDocument, Written},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};

fn invalid(reason: impl ToString) -> PipelineError {
    PipelineError::InvalidInput {
        reason: reason.to_string(),
    }
}

/// Maps bounded Pandoc JSON and its optional extracted-media ZIP into IR.
pub fn read(
    bytes: &[u8],
    media: &[u8],
    source: Format,
    limits: &Limits,
) -> Result<ReadDocument, PipelineError> {
    limits.validate()?;
    if bytes.len() as u64 > limits.max_ir_json_bytes {
        return Err(invalid("Pandoc JSON exceeds the byte budget"));
    }
    prescan_json_depth(
        bytes,
        json_depth_budget_for_nesting(limits.max_nesting_depth),
    )
    .map_err(invalid)?;
    let mut de = serde_json::Deserializer::from_slice(bytes);
    de.disable_recursion_limit();
    let ast = super::ast::Pandoc::deserialize(&mut de).map_err(invalid)?;
    de.end().map_err(invalid)?;
    let mapped = super::to_ir(&ast, limits).map_err(invalid)?;
    let mut document = mapped.document;
    document.meta.source_format = Some(source);
    let mut warnings: Vec<Notice> = mapped.warnings.iter().map(Notice::from).collect();
    let files = media_files(media, limits)?;
    embed_blocks(
        &mut document.body,
        &files,
        &mut document.assets,
        &mut warnings,
    );
    Ok(ReadDocument { document, warnings })
}

/// Maps bounded IR JSON to Pandoc JSON, retaining mapper warnings and title fallback.
pub fn write(bytes: &[u8], title: Option<&str>, limits: &Limits) -> Result<Written, PipelineError> {
    let mut document = pipeline::read_ir_json(bytes, limits)?;
    if document.meta.title.is_none() {
        document.meta.title = title.map(str::to_owned);
    }
    let mapped = super::from_ir(&document);
    Ok(Written {
        bytes: serde_json::to_vec(&mapped.pandoc).map_err(invalid)?,
        warnings: mapped.warnings.iter().map(Notice::from).collect(),
    })
}

type Media = std::collections::BTreeMap<String, Asset>;
fn media_files(bytes: &[u8], limits: &Limits) -> Result<Media, PipelineError> {
    let mut files = Media::new();
    if bytes.is_empty() {
        return Ok(files);
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    if zip.len() as u64 > u64::from(limits.max_archive_entries) {
        return Err(invalid("extracted media exceeds the archive entry budget"));
    }
    let max_asset_bytes = limits.max_asset_bytes.unwrap_or(64 * 1024 * 1024);
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(invalid)?;
        let entry_limit = max_asset_bytes.min(limits.max_decompressed_bytes - total);
        if entry.size() > entry_limit {
            return Err(invalid("extracted media exceeds the byte budget"));
        }
        let name = entry.name().map_err(invalid)?.into_owned();
        if name.split('/').any(|part| part == "..")
            || name.starts_with('/')
            || name.contains(['\\', ':'])
        {
            return Err(invalid("invalid extracted media path"));
        }
        if entry.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        (&mut entry)
            .take(entry_limit.saturating_add(1))
            .read_to_end(&mut data)
            .map_err(invalid)?;
        if data.len() as u64 > entry_limit {
            return Err(invalid("extracted media exceeds the byte budget"));
        }
        total += data.len() as u64;
        let kind = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if data.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else if data.starts_with(b"GIF8") {
            "image/gif"
        } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
            "image/webp"
        } else {
            continue;
        };
        files.insert(
            name,
            Asset {
                media_type: kind.to_owned(),
                bytes: data,
            },
        );
    }
    Ok(files)
}
fn embed(
    target: &mut AssetRef,
    files: &Media,
    assets: &mut AssetStore,
    warnings: &mut Vec<Notice>,
) {
    let AssetRef::Url { href } = target else {
        return;
    };
    let name = href
        .strip_prefix("media.zip/")
        .unwrap_or(href)
        .trim_start_matches("./");
    if let Some(asset) = files.get(name) {
        let id = hex::encode(Sha256::digest(&asset.bytes));
        assets.insert(id.clone(), asset.clone());
        *target = AssetRef::Asset { id };
    } else {
        warnings.push(Notice::new(
            "image_not_embedded",
            "An image could not be embedded; only the selected document is available.",
        ));
    }
}
fn embed_inlines(
    inlines: &mut [Inline],
    files: &Media,
    assets: &mut AssetStore,
    warnings: &mut Vec<Notice>,
) {
    for inline in inlines {
        match inline {
            Inline::Image { target, .. } => embed(target, files, assets, warnings),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => embed_inlines(content, files, assets, warnings),
            _ => {}
        }
    }
}
fn embed_blocks(
    blocks: &mut [Block],
    files: &Media,
    assets: &mut AssetStore,
    warnings: &mut Vec<Notice>,
) {
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                embed_inlines(content, files, assets, warnings)
            }
            Block::Quote { blocks } | Block::Footnote { blocks, .. } => {
                embed_blocks(blocks, files, assets, warnings)
            }
            Block::List { items, .. } => {
                for item in items {
                    embed_blocks(&mut item.blocks, files, assets, warnings);
                }
            }
            Block::Table {
                caption,
                head,
                body,
                footnotes,
                ..
            } => {
                if let Some(caption) = caption {
                    embed_inlines(caption, files, assets, warnings);
                }
                embed_inlines(footnotes, files, assets, warnings);
                for row in head.iter_mut().chain(body.iter_mut()) {
                    for cell in row {
                        embed_blocks(&mut cell.blocks, files, assets, warnings);
                    }
                }
            }
            Block::Figure { asset, caption } => {
                embed(asset, files, assets, warnings);
                embed_inlines(caption, files, assets, warnings);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

    fn media_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn rejects_media_over_the_entry_budget_even_when_not_images() {
        let bytes = media_zip(&[("first.txt", b""), ("second.txt", b"")]);
        let limits = Limits {
            max_archive_entries: 1,
            ..Limits::local()
        };
        assert!(media_files(&bytes, &limits).is_err());
    }

    #[test]
    fn enforces_asset_and_total_decompressed_budgets() {
        let bytes = media_zip(&[("image.png", PNG)]);
        let limits = Limits {
            max_asset_bytes: Some(PNG.len() as u64 - 1),
            ..Limits::local()
        };
        assert!(media_files(&bytes, &limits).is_err());

        let bytes = media_zip(&[("ignored.txt", b"abc"), ("image.png", PNG)]);
        let mut limits = Limits {
            max_decompressed_bytes: PNG.len() as u64 + 2,
            ..Limits::local()
        };
        assert!(media_files(&bytes, &limits).is_err());
        limits.max_decompressed_bytes += 1;
        let files = media_files(&bytes, &limits).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files["image.png"].bytes, PNG);
    }

    #[test]
    fn accepts_the_largest_explicit_byte_limits_without_overflow() {
        let bytes = media_zip(&[("image.png", PNG)]);
        let limits = Limits {
            max_asset_bytes: Some(u64::MAX),
            max_decompressed_bytes: u64::MAX,
            ..Limits::local()
        };
        assert_eq!(media_files(&bytes, &limits).unwrap().len(), 1);
    }

    #[test]
    fn rejects_unsafe_media_paths() {
        for name in [
            "../image.png",
            "/image.png",
            "dir/../image.png",
            "C:image.png",
        ] {
            let bytes = media_zip(&[(name, PNG)]);
            assert!(media_files(&bytes, &Limits::local()).is_err(), "{name}");
        }
    }

    #[test]
    fn read_embeds_packaged_images_and_warns_for_missing_images() {
        let pandoc = serde_json::json!({
            "pandoc-api-version": [1, 23],
            "meta": {},
            "blocks": [{"t": "Para", "c": [
                {"t": "Image", "c": [["", [], []], [], ["media.zip/image.png", ""]]},
                {"t": "Image", "c": [["", [], []], [], ["missing.png", ""]]}
            ]}]
        });
        let bytes = serde_json::to_vec(&pandoc).unwrap();
        let media = media_zip(&[("image.png", PNG)]);
        let result = read(&bytes, &media, Format::Docx, &Limits::local()).unwrap();
        let id = hex::encode(Sha256::digest(PNG));
        assert_eq!(result.document.meta.source_format, Some(Format::Docx));
        assert_eq!(result.document.assets[&id].media_type, "image/png");
        assert_eq!(result.document.assets[&id].bytes, PNG);
        let Block::Paragraph { content } = &result.document.body[0] else {
            panic!("expected paragraph");
        };
        assert!(
            matches!(&content[0], Inline::Image { target: AssetRef::Asset { id: embedded }, .. } if embedded == &id)
        );
        assert!(
            matches!(&content[1], Inline::Image { target: AssetRef::Url { href }, .. } if href == "missing.png")
        );
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, "image_not_embedded");
    }
}
