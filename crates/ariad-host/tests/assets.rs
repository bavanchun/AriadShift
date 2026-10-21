use std::{fs, path::Path};
#[cfg(unix)]
use std::{path::PathBuf, process::Command, sync::mpsc, thread, time::Duration};

use ariad_core::{
    ir::{AssetRef, Block, Document, Inline, ListItem, TableCell},
    limits::Limits,
    warning::WarningCode,
};
use ariad_host::assets;
use sha2::{Digest, Sha256};
use tempfile::{Builder, tempdir};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage payload";

#[test]
fn embeds_a_relative_png_as_a_sha256_asset() {
    let base = tempdir().unwrap();
    fs::write(base.path().join("image.png"), PNG).unwrap();
    let mut document = document_with_images(["image.png"]);

    let warnings = assets::resolve(&mut document, base.path(), &Limits::local()).unwrap();

    assert!(warnings.is_empty());
    let AssetRef::Asset { id } = image_target(&document, 0) else {
        panic!("relative image should be embedded");
    };
    assert_eq!(id, &hex::encode(Sha256::digest(PNG)));
    assert_eq!(document.assets[id].media_type, "image/png");
    assert_eq!(document.assets[id].bytes, PNG);
}

#[test]
fn bare_filename_works_when_the_source_parent_is_empty() {
    let file = Builder::new()
        .prefix("ariad-image-")
        .suffix(".png")
        .tempfile_in(".")
        .unwrap();
    fs::write(file.path(), PNG).unwrap();
    let href = file.path().file_name().unwrap().to_str().unwrap();
    let mut document = document_with_images([href]);

    let warnings = assets::resolve(&mut document, Path::new(""), &Limits::local()).unwrap();

    assert!(warnings.is_empty());
    assert!(matches!(image_target(&document, 0), AssetRef::Asset { .. }));
}

#[test]
fn hostile_hrefs_are_rejected_lexically_without_embedding_outside_files() {
    let root = tempdir().unwrap();
    let base = root.path().join("input");
    fs::create_dir(&base).unwrap();
    fs::write(root.path().join("escape.png"), PNG).unwrap();
    fs::write(root.path().join("absolute.png"), PNG).unwrap();
    let absolute = root
        .path()
        .join("absolute.png")
        .to_string_lossy()
        .into_owned();
    let hrefs = [
        "../escape.png".to_owned(),
        absolute,
        r"C:\x.png".to_owned(),
        r"\\host\share\x.png".to_owned(),
        "file:///x".to_owned(),
        "%2e%2e/escape.png".to_owned(),
        "folder//image.png".to_owned(),
    ];
    let mut document = document_with_images(hrefs.iter().map(String::as_str));

    let warnings = assets::resolve(&mut document, &base, &Limits::local()).unwrap();

    assert_eq!(warnings.len(), hrefs.len());
    assert!(
        warnings
            .iter()
            .all(|warning| warning.code == WarningCode::ImageNotEmbedded)
    );
    assert!(document.assets.is_empty());
    for (index, href) in hrefs.iter().enumerate() {
        assert_eq!(image_href(&document, index), href);
    }
}

#[test]
fn remote_urls_are_preserved_with_a_warning() {
    let mut document = document_with_images(["https://example.test/image.png"]);

    let warnings = assets::resolve(&mut document, Path::new("."), &Limits::local()).unwrap();

    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("URL scheme"));
    assert_eq!(image_href(&document, 0), "https://example.test/image.png");
}

#[test]
fn refuses_an_image_over_the_configured_asset_limit() {
    let base = tempdir().unwrap();
    fs::write(base.path().join("large.png"), PNG).unwrap();
    let mut document = document_with_images(["large.png"]);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(8);

    let warnings = assets::resolve(&mut document, base.path(), &limits).unwrap();

    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("max_asset_bytes"));
    assert_eq!(image_href(&document, 0), "large.png");
    assert!(document.assets.is_empty());
}

#[test]
fn refuses_files_that_do_not_have_supported_image_signatures() {
    let base = tempdir().unwrap();
    fs::write(base.path().join("text.png"), b"not an image").unwrap();
    let mut document = document_with_images(["text.png"]);

    let warnings = assets::resolve(&mut document, base.path(), &Limits::local()).unwrap();

    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("supported image format"));
    assert_eq!(image_href(&document, 0), "text.png");
    assert!(document.assets.is_empty());
}

#[test]
fn finds_an_nfd_filename_from_an_nfc_href() {
    let base = tempdir().unwrap();
    let nfd_name = "cafe\u{301}.png";
    fs::write(base.path().join(nfd_name), PNG).unwrap();
    let mut document = document_with_images(["café.png"]);

    let warnings = assets::resolve(&mut document, base.path(), &Limits::local()).unwrap();

    assert!(warnings.is_empty());
    assert!(matches!(image_target(&document, 0), AssetRef::Asset { .. }));
}

#[test]
fn resolves_images_nested_in_quotes_tables_figures_and_inline_wrappers() {
    let base = tempdir().unwrap();
    fs::write(base.path().join("nested.png"), PNG).unwrap();
    let image = || Inline::Image {
        target: AssetRef::Url {
            href: "nested.png".to_owned(),
        },
        alt: String::new(),
        title: None,
    };
    let figure = Block::Figure {
        asset: AssetRef::Url {
            href: "nested.png".to_owned(),
        },
        caption: vec![Inline::Link {
            url: "#caption".to_owned(),
            title: None,
            content: vec![Inline::Strong {
                content: vec![image()],
            }],
        }],
    };
    let mut document = Document {
        body: vec![Block::Quote {
            blocks: vec![Block::List {
                ordered: false,
                start: None,
                tight: true,
                items: vec![ListItem {
                    checked: None,
                    blocks: vec![Block::Table {
                        caption: Some(vec![image()]),
                        columns: Vec::new(),
                        head: vec![vec![TableCell {
                            rowspan: 1,
                            colspan: 1,
                            header: None,
                            blocks: vec![figure],
                        }]],
                        body: Vec::new(),
                        footnotes: Vec::new(),
                    }],
                }],
            }],
        }],
        ..Document::default()
    };

    let warnings = assets::resolve(&mut document, base.path(), &Limits::local()).unwrap();

    assert!(warnings.is_empty());
    assert_eq!(document.assets.len(), 1);
    let Block::Quote { blocks } = &document.body[0] else {
        panic!("expected quote");
    };
    let Block::List { items, .. } = &blocks[0] else {
        panic!("expected list");
    };
    let Block::Table { caption, head, .. } = &items[0].blocks[0] else {
        panic!("expected table");
    };
    assert!(matches!(
        image_target_in_inline(&caption.as_ref().unwrap()[0]),
        AssetRef::Asset { .. }
    ));
    let Block::Figure { asset, caption } = &head[0][0].blocks[0] else {
        panic!("expected figure");
    };
    assert!(matches!(asset, AssetRef::Asset { .. }));
    let Inline::Link { content, .. } = &caption[0] else {
        panic!("expected link in figure caption");
    };
    let Inline::Strong { content } = &content[0] else {
        panic!("expected strong inline");
    };
    assert!(matches!(
        image_target_in_inline(&content[0]),
        AssetRef::Asset { .. }
    ));
}

#[test]
fn sniffs_png_jpeg_gif_and_webp() {
    let base = tempdir().unwrap();
    let samples = [
        ("image.png", b"\x89PNG\r\n\x1a\n".as_slice(), "image/png"),
        ("image.jpg", b"\xff\xd8\xff".as_slice(), "image/jpeg"),
        ("image.gif", b"GIF89a".as_slice(), "image/gif"),
        (
            "image.webp",
            b"RIFF\x00\x00\x00\x00WEBP".as_slice(),
            "image/webp",
        ),
    ];
    for (name, bytes, _) in samples {
        fs::write(base.path().join(name), bytes).unwrap();
    }
    let mut document = document_with_images(samples.map(|(name, _, _)| name));

    let warnings = assets::resolve(&mut document, base.path(), &Limits::local()).unwrap();

    assert!(warnings.is_empty());
    for (index, (_, _, media_type)) in samples.iter().enumerate() {
        let AssetRef::Asset { id } = image_target(&document, index) else {
            panic!("supported image should be embedded");
        };
        assert_eq!(document.assets[id].media_type, *media_type);
    }
}

#[cfg(unix)]
#[test]
fn refuses_a_symlink_that_leaves_the_base_directory() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let base = root.path().join("input");
    let outside = root.path().join("outside");
    fs::create_dir(&base).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("image.png"), PNG).unwrap();
    symlink(&outside, base.join("linked")).unwrap();
    let mut document = document_with_images(["linked/image.png"]);

    let warnings = assets::resolve(&mut document, &base, &Limits::local()).unwrap();

    assert_eq!(warnings.len(), 1);
    assert_eq!(image_href(&document, 0), "linked/image.png");
    assert!(document.assets.is_empty());
}

#[cfg(unix)]
#[test]
fn refuses_a_fifo_without_blocking() {
    let base = tempdir().unwrap();
    let fifo = base.path().join("pipe.png");
    let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(status.success());
    let base_path = PathBuf::from(base.path());
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut document = document_with_images(["pipe.png"]);
        let result = assets::resolve(&mut document, &base_path, &Limits::local());
        let _ = sender.send((result, document));
    });

    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok((result, document)) => {
            let warnings = result.unwrap();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].message.contains("regular file"));
            assert_eq!(image_href(&document, 0), "pipe.png");
            worker.join().unwrap();
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // If a regression blocks while opening the FIFO, provide a writer so the worker can
            // unwind before failing the test.
            let _writer = fs::OpenOptions::new().write(true).open(&fifo).unwrap();
            worker.join().unwrap();
            panic!("asset resolver blocked while opening a FIFO");
        }
        Err(error) => panic!("asset resolver worker stopped unexpectedly: {error}"),
    }
}

#[cfg(windows)]
#[test]
fn refuses_windows_device_names_lexically() {
    for name in ["CON.png", "NUL"] {
        let mut document = document_with_images([name]);
        let warnings = assets::resolve(&mut document, Path::new("."), &Limits::local()).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("reserved Windows device name"));
        assert_eq!(image_href(&document, 0), name);
    }
}

fn document_with_images<'a>(hrefs: impl IntoIterator<Item = &'a str>) -> Document {
    Document {
        body: hrefs
            .into_iter()
            .map(|href| Block::Paragraph {
                content: vec![Inline::Image {
                    target: AssetRef::Url {
                        href: href.to_owned(),
                    },
                    alt: String::new(),
                    title: None,
                }],
            })
            .collect(),
        ..Document::default()
    }
}

fn image_target(document: &Document, index: usize) -> &AssetRef {
    let Block::Paragraph { content } = &document.body[index] else {
        panic!("expected image paragraph");
    };
    let Inline::Image { target, .. } = &content[0] else {
        panic!("expected image inline");
    };
    target
}

fn image_href(document: &Document, index: usize) -> &str {
    let AssetRef::Url { href } = image_target(document, index) else {
        panic!("image should remain a URL");
    };
    href
}

fn image_target_in_inline(inline: &Inline) -> &AssetRef {
    let Inline::Image { target, .. } = inline else {
        panic!("expected image inline");
    };
    target
}
