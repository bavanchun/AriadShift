mod support;

use std::{fs, path::PathBuf, process::Command};

use serde_json::Value;

fn ashift_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ashift"))
}

fn pdfium_pack_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ASHIFT_PACK_PDFIUM") {
        let path = PathBuf::from(dir);
        if path.join("pack.toml").is_file() {
            return Some(path);
        }
    }
    let candidate = support::fixtures::repository_root().join("target/packs/pdfium");
    if candidate.join("pack.toml").is_file() {
        return Some(candidate);
    }
    None
}

#[test]
fn missing_pdfium_pack_fails_every_pdfium_tool_and_split_pixel_with_exit_5_and_install_hint() {
    let temp = tempfile::tempdir().expect("tempdir");
    let empty_data = temp.path().join("empty-data");
    fs::create_dir_all(&empty_data).unwrap();
    let out_dir = temp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();

    let pdf_fixture =
        support::fixtures::repository_root().join("fixtures/pdf-tools/blank-separators.pdf");

    for (tool, extra_params) in [
        ("pdf.render", &[][..]),
        ("pdf.to_images", &[][..]),
        ("pdf.thumbnails", &[][..]),
        ("pdf.extract_images", &[][..]),
        ("pdf.text_layout", &[][..]),
        ("pdf.remove_blank", &[][..]),
        ("pdf.crop_whitespace", &[][..]),
        ("pdf.detect_scanned", &[][..]),
        (
            "pdf.split",
            &["--param", "mode=blank-separator", "--param", "detect=pixel"][..],
        ),
    ] {
        let output = ashift_cmd()
            .env("ASHIFT_DATA_DIR", &empty_data)
            .env_remove("ASHIFT_PACK_PDFIUM")
            .env_remove("ARIAD_TEST_PDFIUM_DIR")
            .arg("tool")
            .arg(tool)
            .arg(&pdf_fixture)
            .arg("-o")
            .arg(&out_dir)
            .args(extra_params)
            .output()
            .expect("run ashift tool");

        assert_eq!(
            output.status.code(),
            Some(5),
            "{tool} without pdfium pack must exit with code 5; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("pdfium") && stderr.contains("ashift engines install pdfium"),
            "{tool} stderr must name pdfium and its install hint, got: {stderr}"
        );
    }

    // By contrast, pdf.split with detect=content and image.to_pdf run in-process without the pack.
    let split_content_out = temp.path().join("split-content");
    fs::create_dir_all(&split_content_out).unwrap();
    let split_ok = ashift_cmd()
        .env("ASHIFT_DATA_DIR", &empty_data)
        .env_remove("ASHIFT_PACK_PDFIUM")
        .args([
            "tool",
            "pdf.split",
            pdf_fixture.to_str().unwrap(),
            "-o",
            split_content_out.to_str().unwrap(),
            "--param",
            "mode=blank-separator",
            "--param",
            "detect=content",
            "--json",
        ])
        .output()
        .expect("run pdf.split content");
    assert!(
        split_ok.status.success(),
        "pdf.split detect=content must succeed without pdfium pack: {}",
        String::from_utf8_lossy(&split_ok.stderr)
    );

    let png_fixture =
        support::fixtures::repository_root().join("fixtures/md/assets/vietnamese-route.png");
    let img_pdf_out = temp.path().join("from-image.pdf");
    let img_ok = ashift_cmd()
        .env("ASHIFT_DATA_DIR", &empty_data)
        .env_remove("ASHIFT_PACK_PDFIUM")
        .args([
            "tool",
            "image.to_pdf",
            png_fixture.to_str().unwrap(),
            "-o",
            img_pdf_out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run image.to_pdf");
    assert!(
        img_ok.status.success(),
        "image.to_pdf must succeed without pdfium pack: {}",
        String::from_utf8_lossy(&img_ok.stderr)
    );
    assert!(img_pdf_out.is_file());
}

#[test]
fn cli_runs_every_pdfium_tool_end_to_end_when_pack_is_configured() {
    let Some(pack_dir) = pdfium_pack_dir() else {
        return;
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let blank_sep =
        support::fixtures::repository_root().join("fixtures/pdf-tools/blank-separators.pdf");
    let vi_subset = support::fixtures::repository_root()
        .join("fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    let scanned_vi =
        support::fixtures::repository_root().join("fixtures/pdf-tools/scanned-skewed-vi.pdf");

    // 1. pdf.render (PNG and WebP)
    let render_dir = temp.path().join("render");
    fs::create_dir_all(&render_dir).unwrap();
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.render",
            blank_sep.to_str().unwrap(),
            "-o",
            render_dir.to_str().unwrap(),
            "--param",
            "pages=1-2",
            "--param",
            "format=webp",
            "--param",
            "dpi=72",
            "--json",
        ])
        .output()
        .expect("run pdf.render");
    assert!(
        out.status.success(),
        "pdf.render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json_out: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json_out["artifacts"].as_array().unwrap().len(), 2);
    assert!(render_dir.join("blank-separators.page-001.webp").is_file());

    // 2. pdf.to_images (vertical strip)
    let strip_path = temp.path().join("strip.png");
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.to_images",
            blank_sep.to_str().unwrap(),
            "-o",
            strip_path.to_str().unwrap(),
            "--param",
            "pages=1-2",
            "--param",
            "combine=vertical",
            "--param",
            "dpi=72",
            "--json",
        ])
        .output()
        .expect("run pdf.to_images");
    assert!(
        out.status.success(),
        "pdf.to_images failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(strip_path.is_file());

    // 3. pdf.thumbnails (emits PNGs + .thumbnails.json)
    let thumbs_dir = temp.path().join("thumbs");
    fs::create_dir_all(&thumbs_dir).unwrap();
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.thumbnails",
            blank_sep.to_str().unwrap(),
            "-o",
            thumbs_dir.to_str().unwrap(),
            "--param",
            "pages=1-2",
            "--param",
            "max_px=128",
            "--json",
        ])
        .output()
        .expect("run pdf.thumbnails");
    assert!(
        out.status.success(),
        "pdf.thumbnails failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(thumbs_dir.join("blank-separators.page-001.png").is_file());
    assert!(
        thumbs_dir
            .join("blank-separators.thumbnails.json")
            .is_file()
    );

    // 4. pdf.extract_images (format=png -> .page-001.image-001.png)
    let extract_dir = temp.path().join("extract");
    fs::create_dir_all(&extract_dir).unwrap();
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.extract_images",
            scanned_vi.to_str().unwrap(),
            "-o",
            extract_dir.to_str().unwrap(),
            "--param",
            "format=png",
            "--json",
        ])
        .output()
        .expect("run pdf.extract_images");
    assert!(
        out.status.success(),
        "pdf.extract_images failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        extract_dir
            .join("scanned-skewed-vi.page-001.image-001.png")
            .is_file()
    );

    // 5. pdf.text_layout
    let layout_path = temp.path().join("layout.json");
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.text_layout",
            vi_subset.to_str().unwrap(),
            "-o",
            layout_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run pdf.text_layout");
    assert!(
        out.status.success(),
        "pdf.text_layout failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let layout_doc: Value = serde_json::from_slice(&fs::read(&layout_path).unwrap()).unwrap();
    assert_eq!(layout_doc["schema"], "ariad-pdf-text-layout/1");

    // 6. pdf.detect_scanned
    let scan_path = temp.path().join("scan.json");
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.detect_scanned",
            scanned_vi.to_str().unwrap(),
            "-o",
            scan_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run pdf.detect_scanned");
    assert!(
        out.status.success(),
        "pdf.detect_scanned failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let scan_doc: Value = serde_json::from_slice(&fs::read(&scan_path).unwrap()).unwrap();
    assert_eq!(scan_doc["verdict"], "scanned");

    // 7. pdf.remove_blank
    let trimmed_path = temp.path().join("trimmed.pdf");
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.remove_blank",
            blank_sep.to_str().unwrap(),
            "-o",
            trimmed_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run pdf.remove_blank");
    assert!(
        out.status.success(),
        "pdf.remove_blank failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rb_json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rb_json["metrics"]["pages_before"], 6);
    assert_eq!(rb_json["metrics"]["pages_after"], 4);
    assert!(trimmed_path.is_file());

    // 8. pdf.split (mode=blank-separator, detect=pixel)
    let split_dir = temp.path().join("split-pixel");
    fs::create_dir_all(&split_dir).unwrap();
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.split",
            blank_sep.to_str().unwrap(),
            "-o",
            split_dir.to_str().unwrap(),
            "--param",
            "mode=blank-separator",
            "--param",
            "detect=pixel",
            "--json",
        ])
        .output()
        .expect("run pdf.split pixel");
    assert!(
        out.status.success(),
        "pdf.split detect=pixel failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let split_json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(split_json["artifacts"].as_array().unwrap().len(), 2);
    assert!(split_dir.join("blank-separators.part-001.pdf").is_file());
    assert!(split_dir.join("blank-separators.part-002.pdf").is_file());

    // 9. pdf.crop_whitespace
    let cropped_path = temp.path().join("cropped.pdf");
    let out = ashift_cmd()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.crop_whitespace",
            vi_subset.to_str().unwrap(),
            "-o",
            cropped_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run pdf.crop_whitespace");
    assert!(
        out.status.success(),
        "pdf.crop_whitespace failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(cropped_path.is_file());
}
