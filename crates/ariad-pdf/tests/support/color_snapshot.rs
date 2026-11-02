use ariad_core::limits::Limits;
use serde_json::json;
use std::path::Path;

pub fn assert_info_snapshot(name: &str, bytes: &[u8]) {
    let info = ariad_pdf::info::run(bytes, &Limits::local()).unwrap();
    let summary = json!({ "version": info.version, "page_count": info.page_count, "page_1": info.pages[0], "has_annotations": info.has_annotations, "has_forms": info.has_forms, "outline_count": info.outline_count });
    let body = format!(
        "---\nsource: crates/ariad-pdf/tests/{name}.rs\nexpression: info_json\n---\n{}\n",
        serde_json::to_string_pretty(&summary).unwrap()
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}__info.snap"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &body).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), body);
}
