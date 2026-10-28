use std::{fs, io::Read, path::Path};

use ariad_core::{limits::Limits, planner::Profile};
use ariad_host::convert::{ConvertRequest, convert_with_limits};
use base64::{Engine, engine::general_purpose::STANDARD};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use zip::ZipArchive;

#[test]
fn local_docx_conversion_preserves_a_large_data_uri_image() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut image = fs::read(root.join("fixtures/md/assets/vietnamese-route.png")).unwrap();
    // Keep the fixture's PNG data and pad the asset beyond the stamping cap.
    image.resize(65 * 1024 * 1024, 0);
    let directory = tempdir().unwrap();
    let input = directory.path().join("large-image.md");
    let output = directory.path().join("large-image.docx");
    fs::write(
        &input,
        format!(
            "![large image](data:image/png;base64,{})\n",
            STANDARD.encode(&image)
        ),
    )
    .unwrap();

    let request = ConvertRequest {
        input,
        output: output.clone(),
        target_format: "docx".to_owned(),
        profile: Profile::Editable,
        overwrite: false,
        engine_program: env!("CARGO_BIN_EXE_ashift").into(),
        title_fallback: None,
        asset_base_dir: None,
        reader_engine: None,
    };
    convert_with_limits(&request, Limits::local(), CancellationToken::new(), |_| {})
        .expect("convert Markdown with large embedded image under local limits");

    let mut archive = ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let media: Vec<_> = archive
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .filter(|name| name.starts_with("word/media/"))
        .collect();
    assert_eq!(media.len(), 1);
    let mut copied = Vec::new();
    archive
        .by_name(&media[0])
        .unwrap()
        .read_to_end(&mut copied)
        .unwrap();
    assert!(copied == image, "embedded media bytes changed");
}
