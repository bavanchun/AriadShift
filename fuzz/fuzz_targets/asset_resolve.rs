#![no_main]

//! Local image embedding (`ariad_host::assets::resolve`) for hrefs taken from an untrusted
//! Markdown document. The base directory holds real images, a subdirectory and, on Unix,
//! symbolic links that point outside it; a secret image sits next to the base directory.
//! No href may embed the secret, through `..`, percent-encoding, links or any other spelling.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use ariad_core::{
    ir::{AssetRef, AssetStore, Block, Document, Inline},
    limits::Limits,
    reader::markdown,
};
use ariad_host::assets::resolve;
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 32 * 1024;
const MAX_ASSET_BYTES: u64 = 4096;
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

fn png(marker: &[u8]) -> Vec<u8> {
    [PNG_MAGIC, marker].concat()
}

fn secret() -> Vec<u8> {
    png(b"ariad-fuzz secret outside the base directory")
}

/// Builds the per-process directory tree once and returns the base directory.
fn base_dir() -> &'static PathBuf {
    static BASE: OnceLock<PathBuf> = OnceLock::new();
    BASE.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("ariad-fuzz-asset-resolve-{}", std::process::id()));
        let base = root.join("base");
        fs::create_dir_all(base.join("sub")).expect("create fuzz base directory");
        fs::write(root.join("secret.png"), secret()).expect("write secret image");
        fs::write(base.join("img.png"), png(b"inside")).expect("write image");
        fs::write(base.join("sub").join("pic.png"), png(b"nested")).expect("write nested image");
        fs::write(base.join("big.png"), png(&[b'x'; MAX_ASSET_BYTES as usize])).expect("write");
        fs::write(base.join("note.txt"), b"not an image").expect("write text file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let _ = symlink(root.join("secret.png"), base.join("abs-link.png"));
            let _ = symlink("../secret.png", base.join("rel-link.png"));
            let _ = symlink("..", base.join("up"));
            let _ = symlink("img.png", base.join("inner-link.png"));
        }
        base
    })
}

fn check_inlines(inlines: &[Inline], assets: &AssetStore) {
    for inline in inlines {
        match inline {
            Inline::Image {
                target: AssetRef::Asset { id },
                ..
            } => assert!(assets.contains_key(id), "image names a missing asset"),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => check_inlines(content, assets),
            _ => {}
        }
    }
}

fn check_blocks(blocks: &[Block], assets: &AssetStore) {
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                check_inlines(content, assets);
            }
            Block::Quote { blocks } | Block::Footnote { blocks, .. } => {
                check_blocks(blocks, assets)
            }
            Block::List { items, .. } => {
                for item in items {
                    check_blocks(&item.blocks, assets);
                }
            }
            Block::Table {
                caption,
                head,
                body,
                ..
            } => {
                if let Some(caption) = caption {
                    check_inlines(caption, assets);
                }
                for cell in head.iter().chain(body).flatten() {
                    check_blocks(&cell.blocks, assets);
                }
            }
            Block::Figure { asset, caption } => {
                if let AssetRef::Asset { id } = asset {
                    assert!(assets.contains_key(id), "figure names a missing asset");
                }
                check_inlines(caption, assets);
            }
            _ => {}
        }
    }
}

/// Checks only filesystem assets added by resolve; data-URI assets belong to the reader.
fn check(document: &Document, base: &Path, reader_assets: &AssetStore) {
    let secret = secret();
    for (id, asset) in document
        .assets
        .iter()
        .filter(|(id, _)| !reader_assets.contains_key(*id))
    {
        assert_ne!(asset.bytes, secret, "an href escaped {}", base.display());
        assert!(
            asset.bytes.len() as u64 <= MAX_ASSET_BYTES,
            "asset {id} exceeds the limit"
        );
        assert!(
            asset.bytes.starts_with(PNG_MAGIC),
            "asset {id} is not the sniffed image"
        );
    }
    check_blocks(&document.body, &document.assets);
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let base = base_dir();
    let limits = Limits {
        max_asset_bytes: Some(MAX_ASSET_BYTES),
        max_nesting_depth: 16,
        max_blocks: 512,
        ..Limits::local()
    };
    let text = String::from_utf8_lossy(data);
    let Ok(output) = markdown::read(&text, &limits) else {
        return;
    };
    let mut document = output.document;
    // A pre-embedded secret would share its ID with an escaped file, masking an escape.
    if document
        .assets
        .values()
        .any(|asset| asset.bytes == secret())
    {
        return;
    }
    let reader_assets = document.assets.clone();
    resolve(&mut document, base, &limits).expect("the base directory exists");
    for (id, asset) in &reader_assets {
        assert_eq!(
            document.assets.get(id),
            Some(asset),
            "resolve changed a reader asset"
        );
    }
    check(&document, base, &reader_assets);
});
