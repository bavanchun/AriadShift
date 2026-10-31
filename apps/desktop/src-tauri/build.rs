#[path = "src/command_table.rs"]
mod command_table;

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(command_table::NAMES)),
    )
    .expect("desktop application manifest failed");
}
