#![allow(dead_code)]
use ariad_core::limits::Limits;
use serde_json::Value;
use std::path::Path;

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools")
            .join(name),
    )
    .unwrap()
}
pub fn extracted(bytes: &[u8]) -> String {
    fn visit(value: &Value, out: &mut String) {
        match value {
            Value::Object(fields) => {
                if fields.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(value) = fields.get("text").and_then(Value::as_str)
                {
                    out.push_str(value);
                    out.push('\n');
                }
                for value in fields.values() {
                    visit(value, out);
                }
            }
            Value::Array(values) => {
                for value in values {
                    visit(value, out);
                }
            }
            _ => {}
        }
    }
    let document = ariad_core::reader::pdf::read(bytes, &Limits::local()).unwrap();
    let mut output = String::new();
    visit(
        &serde_json::to_value(document.document).unwrap(),
        &mut output,
    );
    output
}
pub fn png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[
                255, 0, 0, 128, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
            2,
            2,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    bytes
}
