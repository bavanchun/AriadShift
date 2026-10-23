fn main() -> std::io::Result<()> {
    ariad_desktop::bindings::export(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings"),
    )
}
