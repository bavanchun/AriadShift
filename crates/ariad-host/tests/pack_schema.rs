use std::{fs, path::PathBuf};

use schemars::generate::SchemaSettings;
use serde_json::json;

const PACK_SCHEMA_ID: &str = "https://ariadshift.ariadnev.com/schemas/pack.v1.json";

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/pack.v1.json")
}

fn generated_schema() -> String {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<ariad_host::packs::Manifest>();
    schema
        .as_object_mut()
        .expect("manifest schema is an object")
        .insert("$id".to_owned(), json!(PACK_SCHEMA_ID));
    let mut contents = serde_json::to_string_pretty(&schema).expect("schema serializes to JSON");
    contents.push('\n');
    contents
}

#[test]
fn pack_schema_matches_the_generated_manifest_schema() {
    let path = schema_path();
    let expected = generated_schema();

    if std::env::var("ARIAD_BLESS_SCHEMAS").as_deref() == Ok("1") {
        fs::write(&path, expected).expect("write generated pack schema");
        return;
    }

    let actual = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "could not read {}: {error}. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-host --test pack_schema",
            path.display()
        )
    });
    assert_eq!(
        actual.replace("\r\n", "\n"),
        expected,
        "pack schema drifted. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-host --test pack_schema to update it."
    );
}

#[test]
fn a_generated_manifest_validates_against_the_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_path()).expect("read schema"))
            .expect("schema is JSON");
    let manifest = ariad_host::packs::Manifest::parse(
        r#"
schema = "ariad-pack/1"
name = "sample"
version = "1.0.0"
license = "MIT"
requires_ashift = ">=0.1.0"

[[targets]]
triple = "x86_64-unknown-linux-gnu"
entrypoint = ["bin/engine"]
env = { HF_HUB_OFFLINE = "1" }

[[targets.components]]
name = "engine"
url = "engine"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
size = 1
unpack = "file"
executable = true
"#,
    )
    .expect("parse manifest");
    let instance = serde_json::to_value(&manifest).expect("manifest to JSON");
    let validator = jsonschema::validator_for(&schema).expect("compile schema");
    assert!(validator.is_valid(&instance));

    let with_executables = ariad_host::packs::Manifest::parse(
        r#"
schema = "ariad-pack/1"
name = "sample"
version = "1.0.0"
license = "MIT"
requires_ashift = ">=0.1.0"

[[targets]]
triple = "x86_64-unknown-linux-gnu"
entrypoint = ["bin/engine"]
executables = ["bin/engine", "bin/helper"]

[[targets.components]]
name = "runtime"
url = "runtime.tar.zst"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
size = 100
unpack = "tar.zst"
unpacked_size = 200
entries = 3
"#,
    )
    .expect("parse manifest with executables");
    let exec_instance = serde_json::to_value(&with_executables).expect("manifest to JSON");
    assert!(validator.is_valid(&exec_instance));
}
